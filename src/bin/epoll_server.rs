
use nix::sys::epoll::*;
use nix::sys::socket::*;

use std::collections::VecDeque;

const MAX_EVENTS: usize = 128;
const MAX_MESSAGE_SIZE: usize = 1024;
const MAX_QUEUE_SIZE: usize = 64;

struct Message {
    buffer: Vec<u8>,
    addr: SockAddr
}

fn main() {
    // 创建一个 TCP socket
    let socket_fd = socket(
        AddressFamily::Inet, 
        SockType::Stream, 
        SockFlag::SOCK_NONBLOCK, 
        SockProtocol::Tcp
    ).expect("Fail to new socket");

    let localhost: IpAddr = IpAddr::new_v4(127, 0, 0, 1);
    // 绑定端口
    bind(
        socket_fd, 
        &SockAddr::new_inet(InetAddr::new(localhost, 8080))
    ).expect("Fail to bind socket");

    listen(
        socket_fd, 
        10
    ).unwrap();

    // 创建 epoll 事件
    // 可读事件
    let mut event_read_only = EpollEvent::new(EpollFlags::EPOLLIN, 0u64);
    // 可读可写事件
    let mut event_read_write = EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLOUT, 0u64);
    // 回调事件的数组，当 epoll 中有响应事件
    let mut current_events = [EpollEvent::empty(); MAX_EVENTS];
    
    // 注册 epoll
    let epoll_fd = epoll_create1(EpollCreateFlags::empty()).expect("Fail to create epoll");
    // 开始只响应可读事件
    epoll_ctl(
        epoll_fd, 
        EpollOp::EpollCtlAdd, 
        socket_fd, 
        &mut event_read_only
    ).unwrap();

    let mut outgoing_queue: VecDeque<Message> = VecDeque::new();
    loop {
        if outgoing_queue.is_empty() {
            epoll_ctl(
                epoll_fd, 
                EpollOp::EpollCtlMod,
                socket_fd, 
                &mut event_read_only
            ).unwrap();
        }else {
            epoll_ctl(
                epoll_fd, 
                EpollOp::EpollCtlMod, 
                socket_fd, 
                &mut event_read_write
            ).unwrap();
        }

        let num_events = epoll_wait(
            epoll_fd, 
            &mut current_events, 
            -1
        ).unwrap();

        for i in 0..num_events {
            let event = &current_events[i];
            if event.events().contains(EpollFlags::EPOLLIN) {
                let mut buf = [0; MAX_MESSAGE_SIZE];
                let (nbytes, addr) = recvfrom(socket_fd, &mut buf).unwrap();
                println!("recv {} bytes from {}", nbytes, addr.unwrap());

                if outgoing_queue.len() > MAX_QUEUE_SIZE {
                    println!("outgoing buffers exhausted; dropping packet.");
                }else {
                    outgoing_queue.push_back(Message{
                        buffer: buf[0..nbytes].to_vec(),
                        addr: addr.unwrap()
                    });
                    println!("Total pending writes: {}", outgoing_queue.len());
                }
            }

            if event.events().contains(EpollFlags::EPOLLOUT) {
                let msg = outgoing_queue.pop_front().unwrap();
                let nbytes = sendto(
                    socket_fd,
                    &msg.buffer,
                    &msg.addr,
                    MsgFlags::empty()
                ).unwrap();
                println!("Send {} bytes to {}", nbytes, msg.addr);
            }
        }
    }
}