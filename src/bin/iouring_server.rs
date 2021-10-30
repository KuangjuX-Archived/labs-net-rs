use io_uring::{IoUring, SubmissionQueue, opcode, squeue, types};
use slab::Slab;

use std::collections::VecDeque;
use std::net::TcpListener;
use std::os::unix::io::{ AsRawFd, RawFd };
use std::{ io, ptr };

#[derive(Clone, Debug)]
enum Token {
    Accept, 
    Poll {
        fd: RawFd
    },
    Read {
        fd: RawFd,
        buf_index: usize
    },
    Write {
        fd: RawFd, 
        buf_index: usize, 
        offset: usize,
        len: usize
    }
}

pub struct AcceptCount {
    entry: squeue::Entry,
    count: usize
}

impl AcceptCount {
    /// 新建 AcceptCount 结构体,fd 表示监听的文件描述符,token 表示 sqe 携带的用户数据
    /// count 表示该文件描述符所能接收到的最大连接
    fn new(fd: RawFd, token: usize, count: usize) -> Self {
        Self {
            entry: opcode::Accept::new(types::Fd(fd), ptr::null_mut(), ptr::null_mut())
                    .build()
                    .user_data(token as _),
            count
        }
    }

    /// 向提交队列中提交事件
    pub fn push_to(&mut self, sq: &mut SubmissionQueue<'_>) {
        while self.count > 0 {
            unsafe{
                match sq.push(&self.entry) {
                    Ok(_) => self.count -= 1,
                    Err(_) => break,
                }
            }
        }
        sq.sync();
    }
}
fn main() {
    let mut ring = IoUring::new(256).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 8080)).unwrap();

    // 用于存放提交失败的事件
    let mut backlog = VecDeque::new();
    // 用于存放空闲的缓冲区的 buf_index,一般为关闭连接的socket被回收的
    let mut bufpool = Vec::with_capacity(64);
    // 用来存储内存中的缓冲区的指针，使用 buf_index 进行访问
    let mut buf_alloc = Slab::with_capacity(64);
    // 一段用来存放不同事件token的内存区域，通过token_index获取到事件类型及信息
    let mut token_alloc = Slab::with_capacity(64);

    // 用来存放所有建立连接的 sockets
    let mut sockets = Vec::new();

    println!("Server listen on {}", listener.local_addr().unwrap());

    // 从 io_uring 实例中获取提交者,提交队列，完成队列
    let (submitter, mut sq, mut cq) = ring.split();

    // 建立 AcceptCount，用于计算监听的文件描述符并提交事件
    let mut accept = AcceptCount::new(listener.as_raw_fd(), token_alloc.insert(Token::Accept), 10);
    accept.push_to(&mut sq); 

    loop {
        // 提交SQ里的所有队列，等待至少一个事件成功返回
        match submitter.submit_and_wait(1) {
            Ok(_) => (),
            Err(ref err) => if err.raw_os_error() == Some(libc::EBUSY) { break; },
            Err(err) => panic!(err)
        }
        // 同步完成队列，刷新在内核中的CQEs
        cq.sync();

        loop {
            if sq.is_full() {
                // 提交队列满了的时候提交所有任务到内核
                match submitter.submit() {
                    Ok(_) => (),
                    Err(ref err) => if err.raw_os_error() == Some(libc::EBUSY) {break;},
                    Err(err) => panic!(err)
                }
            }
            // 同步提交队列的内容
            sq.sync();

            match backlog.pop_front() {
                Some(sqe) => unsafe {
                    // 向SQ中提交事件（此时没有被提交到内核中）
                    let _ = sq.push(&sqe);
                },

                None => break,
            }
        }

        accept.push_to(&mut sq);

        for cqe in &mut cq {
            // 遍历完成队列的内容
            // 获取 CQE 的结果
            let ret = cqe.result();
            // 获取 CQE 的用户数据（用于判断是什么事件）
            let token_index = cqe.user_data() as usize;

            if ret < 0  {
                // 表明该事件执行失败了
                eprintln!(
                    "token {:?} error: {:?}",
                    token_alloc.get(token_index),
                    io::Error::from_raw_os_error(-ret)
                );
                continue;
            }

            // 通过传入的用户数据取出对应的 token 用于判断是什么事件
            let token = &mut token_alloc[token_index];
            match token.clone() {
                Token::Accept => {
                    // 当接收到客户端连接时，将 accept 的 count 域进行迭代
                    accept.count += 1;
                    // 此时收到的结果是一个文件描述符，表示的是接收到连接的socket
                    let fd = ret;
                    // 将文件描述符push到sockets中
                    sockets.push(fd);
                    // 此时向分配 token_alloc 中插入Token获取token用于作为 user_data
                    let poll_token = token_alloc.insert(Token::Poll{ fd });
                    // 创建poll实例，不断轮询检测是否从该socket中收到信息
                    let poll_e = opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as _)
                                        .build()
                                        .user_data(poll_token as _);
                    unsafe{
                        if sq.push(&poll_e).is_err() {
                            // 如果没有提交到提交队列中(此时应当是提交队列已满)，则将其放入backlog中，等待下一次提交
                            backlog.push_back(poll_e);
                        }
                    }
                }

                Token::Poll { fd } => {
                    let (buf_index, buf) = match bufpool.pop() {
                        Some(buf_index) => (buf_index, &mut buf_alloc[buf_index]),
                        None => {
                            // 新建一个缓冲区
                            let buf = vec![0u8; 2048].into_boxed_slice();
                            // 返回一个空条目的 handle,允许进一步进行操作
                            let buf_entry = buf_alloc.vacant_entry();
                            // 获取该 handle 的key(index)
                            let buf_index = buf_entry.key();
                            // 返回索引和将缓冲区插入 entry中
                            (buf_index, buf_entry.insert(buf))
                        }
                    };

                    *token = Token::Read { fd, buf_index };

                    // 当 Poll 事件返回后表明有一个可读事件发生，此时应当注册读取事件，并将
                    // 该事件 push 到提交队列中
                    let read_e = opcode::Recv::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
                                        .build()
                                        .user_data(token_index as _);

                    unsafe {
                        if sq.push(&read_e).is_err() {
                            backlog.push_back(read_e);
                        }
                    }
                }

                Token::Read { fd, buf_index} => {
                    // 读取事件返回，表明从连接的socket中读取到了传输来的信息
                    if ret == 0 {
                        // 结果为0,表明对方关闭了连接
                        // 此时这个缓冲区就没有用了，将其push
                        // 到 bufpool,用于下一次read/write事件
                        // 作为缓冲区
                        bufpool.push(buf_index);
                        // 将token_index从token_alloc移除掉
                        token_alloc.remove(token_index);

                        println!("shutdown");

                        for i in 0..sockets.len() {
                            if sockets[i] == fd {
                                sockets.remove(i);
                            }
                        }

                        unsafe {
                            libc::close(fd);
                        }
                    }else {
                        // 读取成功，此时的结果表明读取的字节数
                        let len = ret as usize;
                        // 获取用来获取 read 的缓冲区
                        let buf = &buf_alloc[buf_index];

                        let socket_len = sockets.len();
                        // 移除之前的 token_index 并进行重新分配
                        token_alloc.remove(token_index);
                        for i in 0..socket_len {
                            // 新建write_token并将其传输给所有正在连接的socket
                            let write_token = Token::Write {
                                fd: sockets[i], 
                                buf_index,
                                len,
                                offset: 0
                            };

                            let write_token_index = token_alloc.insert(write_token);

                            // 注册 write 事件，实际上是注册 send syscall 的事件
                            let write_e = opcode::Send::new(types::Fd(sockets[i]), buf.as_ptr(), len as _)
                                                .build()
                                                .user_data(write_token_index as _);
                            unsafe {
                                if sq.push(&write_e).is_err() {
                                    backlog.push_back(write_e);
                                }
                            }
                        }

                    }
                }

                Token::Write {
                    fd,
                    buf_index,
                    offset,
                    len
                } => {
                    // write(send) 事件返回，此时的结果是写字节数
                    let write_len = ret as usize; 

                    // 如果写偏移量的写数据的字节数大于等于要写的长度，
                    // 此时表明已经写完，则开始注册等待事件继续轮询socket是否传输信息
                    let entry = if offset + write_len >= len {
                        bufpool.push(buf_index);

                        *token = Token::Poll { fd };

                        opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as _)
                                .build()
                                .user_data(token_index as _)
                    }else {
                        // 如果没写完的话则更新参数重新写
                        // 将写偏移量加上写字节数
                        let offset = offset + write_len;
                        // 将要写的数据长度减去偏移量
                        let len = len - offset;
                        // 通过偏移量获取缓冲区的指针
                        let buf = &buf_alloc[buf_index][offset..];

                        *token = Token::Write {
                            fd, 
                            buf_index,
                            offset, 
                            len
                        };

                        opcode::Write::new(types::Fd(fd), buf.as_ptr(), len as _)
                                    .build()
                                    .user_data(token_index as _)
                    };

                    unsafe {
                        if sq.push(&entry).is_err() {
                            // 将事件push到提交队列中，失败了则放入到备份中
                            backlog.push_back(entry);
                        }
                    }
                }
            }
        }
    }
}