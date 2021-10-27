use std::net::TcpListener;
use std::sync::mpsc;
use std::io::{ErrorKind, Read, Write};
use server::tp::{SharedQueueThreadPool, ThreadPool};

fn main() {
    println!("Hello Echo Server");
    let listener = TcpListener::bind("127.0.0.1:8080").expect("Fail to bind address");
    listener.set_nonblocking(true).unwrap();
    let tp = SharedQueueThreadPool::new(16).expect("Fail to new a thread pool");
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    let mut clients = vec![];
    loop {
        if let Ok((mut socket, addr)) = listener.accept() {
            println!("客户端 {} 连接", addr);
            let sender = sender.clone();
            clients.push(socket.try_clone().unwrap());
            tp.spawn(move || {
                loop {
                    let mut buf = [0; 1024];
                    match socket.read(&mut buf) {
                        Ok(0) => { continue; }
                        Ok(n) => {
                            let s = String::from_utf8(buf.to_vec()).expect("Fail to convert u8 to string");
                            println!("Server receive {} bytes from {}",n ,addr);
                            sender.send(s.into_bytes()).expect("Fail to send message to sender");
                        },
                        
                        Err(ref err) if err.kind() == ErrorKind::WouldBlock => (),
                        Err(_) => {
                            println!("CLient {} exit", addr);
                            break;
                        }
                    }
                }
            })
        }

        if let Ok(msg) =  receiver.try_recv() {
            for client in clients.iter_mut() {
                client.write(&msg).expect("Fail to write");
            }
        }
    }
}

