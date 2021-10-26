use io_uring::{IoUring, SubmissionQueue, opcode, squeue, types};
use slab::Slab;

use std::{collections::VecDeque, net::TcpListener};

pub struct AcceptCount {
    entry: squeue::Entry,
    count: usize
}
fn main() {
    let mut ring = IoUring::new(256).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 8080)).unwrap();

    let mut backlog = VecDeque::new();
    let mut bufpool = Vec::with_capacity(64);
    let mut buf_alloc = Slab::with_capacity(64);
    let mut token_alloc = Slab::with_capacity(64);

    println!("Server listen on {}", listener.local_addr().unwrap());

    let (submitter, mut sq, mut cq) = ring.split();

    
}