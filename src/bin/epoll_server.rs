
use nix::fcntl::*;
use nix::sys::epoll::*;
use nix::sys::socket::*;
use nix::unistd::close;
use nix::unistd::{ write, read };

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use webserver::tp::{ SharedQueueThreadPool, ThreadPool };

const MAX_EVENTS: usize = 128;
struct Message {
    buffer: Vec<u8>,
    addr: SockAddr
}

struct SocketInfo {
    fd: i32,
    addr: SockAddr
}

fn main() {
    // 创建一个 TCP socket
    let listen_fd = socket(
        AddressFamily::Inet, 
        SockType::Stream, 
        SockFlag::SOCK_NONBLOCK, 
        SockProtocol::Tcp
    ).expect("Fail to new socket");

    let localhost: IpAddr = IpAddr::new_v4(127, 0, 0, 1);
    // 绑定端口
    bind(
        listen_fd, 
        &SockAddr::new_inet(InetAddr::new(localhost, 8080))
    ).expect("Fail to bind socket");

    listen(
        listen_fd, 
        1024
    ).unwrap();

    // 创建线程池
    let tp = SharedQueueThreadPool::new(16).unwrap();

    // 创建连接队列
    let connection_sockets = Arc::new(Mutex::new(VecDeque::new()));

    // 创建 epoll 事件
    // 可读事件
    let mut event_read_only = EpollEvent::new(EpollFlags::EPOLLIN, listen_fd as u64);
    // 回调事件的数组，当 epoll 中有响应事件则加入到这个数组中
    let mut current_events = [EpollEvent::empty(); MAX_EVENTS];
    
    // 注册 epoll
    let epoll_fd = epoll_create1(EpollCreateFlags::empty()).expect("Fail to create epoll");
    // 开始只响应可读事件
    epoll_ctl(
        epoll_fd, 
        EpollOp::EpollCtlAdd, 
        listen_fd, 
        &mut event_read_only
    ).unwrap();

    let mut outgoing_queue: Arc<Mutex<VecDeque<Message>>> = Arc::new(Mutex::new(VecDeque::new()));
    loop {
        // 等待事件，返回发生事件数量
        let num_events = epoll_wait(
            epoll_fd, 
            &mut current_events, 
            -1
        ).unwrap();

        // 遍历所有事件
        for i in 0..num_events {
            let event = current_events[i].clone();
            if event.data() as i32 == listen_fd {
                // 如果当前事件发生在 listen_fd 上，则说明产生连接，此时接收连接
                if event.events().contains(EpollFlags::EPOLLIN) {
                    // 接收连接，获取 socket
                    let socket_fd = accept(listen_fd).unwrap();
                    if socket_fd > 0 {
                        // 设置连接为非阻塞模式
                        fcntl(socket_fd, FcntlArg::F_GETFL).unwrap();
                        fcntl(socket_fd, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
                        let addr = getpeername(socket_fd).unwrap();
                        println!("Server accept {}", addr);
                        // 将新连接加入到epoll中，设置读写模式，即当发生可读或可写事件时都会触发
                        // 可读可写事件
                        let mut event_read_write = EpollEvent::new(
                            EpollFlags::EPOLLIN | EpollFlags::EPOLLOUT | EpollFlags::EPOLLET, 
                            socket_fd as u64
                        );
                        let mut conn_guard = connection_sockets.lock().unwrap();
                        conn_guard.push_back(SocketInfo{
                            fd: socket_fd,
                            addr: addr
                        });
                        drop(conn_guard);
                        epoll_ctl(
                            epoll_fd, 
                            EpollOp::EpollCtlAdd, 
                            socket_fd, 
                            &mut event_read_write
                        ).unwrap();
                    }
                }
            }else {
                if event.events().contains(EpollFlags::EPOLLERR | EpollFlags::EPOLLHUP) {
                    // 断开和连接出错，从 epoll 中删除这个文件描述符
                    epoll_ctl(
                        epoll_fd, 
                        EpollOp::EpollCtlDel, 
                        event.data() as i32, 
                        None
                    ).unwrap();
                    close(event.data() as i32).unwrap();
                }else if event.events().contains(EpollFlags::EPOLLIN) {
                    // 当 socket 可读时
                    let mut outgoing_queue = Arc::clone(&outgoing_queue);
                    tp.spawn(move || {
                        // 开启线程池，将消息送入到队列中
                        let mut buf = [0; 1024];
                        // let nbytes = read(event.data() as i32, &mut buf).unwrap();
                        match read(event.data() as i32, &mut buf) {
                            Ok(nbytes) => {
                                let addr = getpeername(event.data() as i32).unwrap();
                                let s = String::from_utf8_lossy(&buf);
                                println!("Client receive {} bytes msg: {}", nbytes, s);
                                let mut guard = outgoing_queue.lock().unwrap();
                                guard.push_back(Message{buffer: buf.to_vec(), addr: addr});
                                drop(guard);
                            },

                            Err(_) => ()
                        }
                    });
                }
                if event.events().contains(EpollFlags::EPOLLOUT) {
                    // 当 socket 可写时
                    let mut outgoing_queue = Arc::clone(&outgoing_queue);
                    let mut connection_sockets = Arc::clone(&connection_sockets);
                    tp.spawn(move || {
                        // 开启线程池，将data从队列中取出并送入客户端
                        let mut guard = outgoing_queue.lock().unwrap();
                        match guard.pop_front() {
                            Some(msg) => {
                                // 将消息发送到客户端
                                let conn_guard = connection_sockets.lock().unwrap();
                                for socket_info in conn_guard.iter() {
                                    let nbytes = write(socket_info.fd, &msg.buffer).unwrap();
                                    println!("Send {} bytes into {}", nbytes, socket_info.addr);
                                }
                                drop(conn_guard);
                            },
                            None => ()
                        }
                        drop(guard);
                    });
                }
            }

        }
    }
}