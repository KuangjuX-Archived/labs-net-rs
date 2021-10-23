use std::net::TcpStream;
use std::io::{ Read, Write, stdin, ErrorKind };
use std::sync::{Arc, Mutex, mpsc};

fn main() {
    let mut client = TcpStream::connect("127.0.0.1:8080").expect("Fail to connect");
    client.set_nonblocking(true).unwrap();
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        loop {
            let mut buffer = [0; 1024];
            match client.read_exact(&mut buffer) {
                Ok(_) => {
                    let msg = String::from_utf8(buffer.to_vec()).unwrap();
                    println!("Client receive message： {}", msg);
                },
                Err(ref err) if err.kind() == ErrorKind::WouldBlock => (),

                Err(err) => {
                    println!("Client close cnnection beacause {}", err);
                    break;
                }
            }

            if let Ok(data) = receiver.try_recv() {
                client.write(&data).expect("Fail to transform data into server");
            }
        }
    });

    loop {
        let mut buffer = String::new();
        stdin().read_line(&mut buffer).unwrap();
        let message = buffer.trim().to_string();
        let mut buf = message.into_bytes();
        buf.resize(1024, 0);
        sender.send(buf).expect("Fail to send message");
    }
}