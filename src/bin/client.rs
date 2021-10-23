use std::net::TcpStream;
use std::io::{ Read, Write, stdin, ErrorKind };
use std::sync::{Arc, Mutex};

fn main() {
    let client = TcpStream::connect("127.0.0.1:8080").expect("Fail to connect");
    client.set_nonblocking(true).unwrap();
    let client = Arc::new(Mutex::new(client));
    let rclient = Arc::clone(&client);
    let wclient = Arc::clone(&client);
    std::thread::spawn(move || {
        loop {
            let mut buffer = [0; 1024];
            let mut guard = rclient.lock().unwrap();
            match guard.read_exact(&mut buffer) {
                Ok(_) => {
                    let msg = String::from_utf8(buffer.to_vec()).unwrap();
                    println!("msg: {}", msg);
                },
                Err(ref err) if err.kind() == ErrorKind::WouldBlock => (),

                Err(err) => {
                    // panic!("err: {}", err);
                    println!("连接中断");
                    break;
                }
            }
            drop(guard);
        }
    });

    loop {
        let mut buffer = String::new();
        // println!(">>>");
        stdin().read_line(&mut buffer).unwrap();
        let message = buffer.trim().to_string();
        println!("Client send msg: {}", message);
        let mut buf = message.into_bytes();
        buf.resize(1024, 0);
        let mut guard = wclient.lock().unwrap();
        guard.write(&buf).unwrap();
        drop(guard);
    }
}