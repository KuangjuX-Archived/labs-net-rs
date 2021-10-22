use std::net::TcpStream;
use std::io::{ Read, Write };

fn main() {
    let mut client = TcpStream::connect("127.0.0.1:8080").expect("Fail to connect");
    let s = "Hello World!";
    println!("Client send {}", s);
    client.write(s.as_bytes()).expect("Fail to write");
    let mut buf = [0;1024];
    client.read_exact(&mut buf).expect("Fail to read");
    println!("Client receive {}", String::from_utf8(buf.to_vec()).unwrap());
}