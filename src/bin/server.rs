use std::net::TcpListener;
use std::sync::mpsc;
use std::io::{ Read, Write };
use webserver::tp::{SharedQueueThreadPool, ThreadPool};

fn main() {
    println!("Hello Echo Server");
    let listener = TcpListener::bind("127.0.0.1:8080").expect("Fail to bind address");
    let tp = SharedQueueThreadPool::new(16).expect("Fail to new a thread pool");
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    let mut clients = vec![];
    loop {
        if let Ok((mut stream, addr)) = listener.accept() {
            println!("Address {} connect", addr);
            let sender = sender.clone();
            clients.push(stream.try_clone().unwrap());
            tp.spawn(move || {
                let mut buf = [0; 1024];
                match stream.read_exact(&mut buf) {
                    Ok(_) => {
                        let s = String::from_utf8(buf.to_vec()).expect("Fail to convert u8 to string");
                        println!("Server receive client {} msg: {}", addr, s);
                        sender.send(s.into_bytes()).expect("Fail to send message to sender");
                    },
    
                    Err(err) => {
                        panic!("err: {}", err);
                    }
                }
            })
        }

        match receiver.recv() {
            Ok(msg) => {
                for client in clients.iter_mut() {
                    client.write(&msg).expect("Fail to write");
                }
            },

            Err(err) => {
                panic!("err: {}", err);
            }
        }
    }
}