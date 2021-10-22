use std::net::TcpStream;
use std::io::{ Read, Write, stdin };
use std::sync::mpsc;

fn main() {
    let mut client = TcpStream::connect("127.0.0.1:8080").expect("Fail to connect");
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    // client.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut buffer = [0; 1024];
        // let receiver = receiver.clone();
        match client.read_exact(&mut buffer) {
            Ok(_) => {
                let msg = String::from_utf8(buffer.to_vec()).unwrap();
                println!("msg: {}", msg);
            },

            Err(err) => {
                panic!("err: {}", err);
            }
        }
        match receiver.try_recv() {
            Ok(msg) => {
                client.write_all(&msg).unwrap();
            },
            Err(err) => {
                panic!("err: {}", err);
            }
        }
    });
    loop {
        let mut buffer = String::new();
        println!(">>>");
        stdin().read_line(&mut buffer).unwrap();
        let message = buffer.trim().to_string();
        println!("Client send msg: {}", message);
        sender.send(message.into_bytes()).unwrap();
    }
}