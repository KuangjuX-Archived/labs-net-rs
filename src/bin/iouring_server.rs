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
    fn new(fd: RawFd, token: usize, count: usize) -> Self {
        Self {
            entry: opcode::Accept::new(types::Fd(fd), ptr::null_mut(), ptr::null_mut())
                    .build()
                    .user_data(token as _),
            count
        }
    }

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

    let mut backlog = VecDeque::new();
    let mut bufpool = Vec::with_capacity(64);
    let mut buf_alloc = Slab::with_capacity(64);
    let mut token_alloc = Slab::with_capacity(64);

    println!("Server listen on {}", listener.local_addr().unwrap());

    // 从 io_uring 时例中获取提交者,提交队列，完成队列
    let (submitter, mut sq, mut cq) = ring.split();

    let mut accept = AcceptCount::new(listener.as_raw_fd(), token_alloc.insert(Token::Accept), 10);
    accept.push_to(&mut sq); 

    loop {
        match submitter.submit_and_wait(1) {
            Ok(_) => (),
            Err(ref err) => if err.raw_os_error() == Some(libc::EBUSY) { break; },
            Err(err) => panic!(err)
        }
        cq.sync();

        loop {
            if sq.is_full() {
                // 提交队列满了的时候提交任务到内核
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
                    let _ = sq.push(&sqe);
                },

                None => break,
            }
        }

        accept.push_to(&mut sq);

        for cqe in &mut cq {
            // 遍历完成队列的内容
            let ret = cqe.result();
            let token_index = cqe.user_data() as usize;

            if ret < 0  {
                eprintln!(
                    "token {:?} error: {:?}",
                    token_alloc.get(token_index),
                    io::Error::from_raw_os_error(-ret)
                );
                continue;
            }

            let token = &mut token_alloc[token_index];
            match token.clone() {
                Token::Accept => {
                    // 当接收到客户端连接时，则将收到的文件描述符插入到 token_alloc 中
                    accept.count += 1;
                    let fd = ret;
                    let poll_token = token_alloc.insert(Token::Poll{ fd });
                    // 创建poll实例，不断轮询检测是否从该socket中收到信息
                    let poll_e = opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as _)
                                        .build()
                                        .user_data(poll_token as _);
                    unsafe{
                        if sq.push(&poll_e).is_err() {
                            // 如果没有提交到提交队列中，则将其放入backlog中，等待下一次提交
                            backlog.push_back(poll_e);
                        }
                    }
                }

                Token::Poll { fd } => {
                    let (buf_index, buf) = match bufpool.pop() {
                        Some(buf_index) => (buf_index, &mut buf_alloc[buf_index]),
                        None => {
                            let buf = vec![0u8; 2048].into_boxed_slice();
                            let buf_entry = buf_alloc.vacant_entry();
                            let buf_index = buf_entry.key();
                            (buf_index, buf_entry.insert(buf))
                        }
                    };

                    *token = Token::Read { fd, buf_index };

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
                    if ret == 0 {
                        bufpool.push(buf_index);
                        token_alloc.remove(token_index);

                        println!("shutdown");

                        unsafe {
                            libc::close(fd);
                        }
                    }else {
                        let len = ret as usize;
                        let buf = &buf_alloc[buf_index];

                        *token = Token::Write {
                            fd, 
                            buf_index,
                            len,
                            offset: 0
                        };

                        let write_e = opcode::Send::new(types::Fd(fd), buf.as_ptr(), len as _)
                                            .build()
                                            .user_data(token_index as _);
                        unsafe {
                            if sq.push(&write_e).is_err() {
                                backlog.push_back(write_e);
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
                    let write_len = ret as usize; 

                    let entry = if offset + write_len >= len {
                        bufpool.push(buf_index);

                        *token = Token::Poll { fd };

                        opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as _)
                                .build()
                                .user_data(token_index as _)
                    }else {
                        let offset = offset + write_len;
                        let len = len - offset;
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
                            backlog.push_back(entry);
                        }
                    }
                }
            }
        }
    }
}