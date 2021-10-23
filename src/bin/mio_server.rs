use mio::{Events, Interest, Poll, Token, net::TcpListener};
use webserver::tp::{ SharedQueueThreadPool, ThreadPool };
use std::collections::HashMap;
use std::io::{Read, Write, ErrorKind};
use std::sync::mpsc;

const SERVER_ACCEPT: Token = Token(0);
// const SERVER: Token = Token(1);
// const CLIENT: Token = Token(2);

fn next(current: &mut Token) -> Token {
    let next = current.0;
    current.0 += 1;
    Token(next)
}

fn main() {
    let addr = "127.0.0.1:8080".parse().unwrap();
    let mut server = TcpListener::bind(addr).unwrap();
    let mut events = Events::with_capacity(1024);
    let mut poll = Poll::new().unwrap();
    // 所有 sockets 使用哈希表来维护
    let mut sockets = HashMap::new();
    // 独特的token，用来生成 Token 并插入哈希表
    // 通道，用于在线程池与主线程之间进行通信
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    let mut unique_token = Token(SERVER_ACCEPT.0 + 1);
    // 注册到 event poll 中
    poll.registry().register(
        &mut server, 
        SERVER_ACCEPT, 
        Interest::READABLE
    ).unwrap();
    // 初始化线程池
    let tp = SharedQueueThreadPool::new(16).unwrap();

    loop {
        poll.poll(&mut events, None).unwrap();
        for event in events.iter() {
           match event.token() {
               SERVER_ACCEPT => {
                let (mut socket, address) = match server.accept() {
                    Ok((socket, address)) => (socket, address),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(e) => {
                        panic!("{}", e);
                    }
                };

                println!("接受连接: {}", address);

                let token = next(&mut unique_token);
                poll.registry().register(
                    &mut socket,
                    token,
                    Interest::READABLE.add(Interest::WRITABLE),
                ).unwrap();

                sockets.insert(token, socket);
               },

               token => {
                    if let Some(socket) = sockets.get_mut(&token) {
                        if event.is_writable() {
                            if let Ok(msg) = receiver.try_recv() {
                                match socket.write(&msg) {
                                    Ok(n) if n < msg.len() => panic!(),
                                    Ok(_) => {
                                        poll.registry().register(
                                            socket, 
                                            event.token(), 
                                            Interest::READABLE
                                        ).unwrap();
                                    }

                                    Err(ref err) if err.kind() == ErrorKind::WouldBlock => {},
                                    Err(ref err) if err.kind() == ErrorKind::Interrupted => {
                                        
                                    },

                                    Err(err) => panic!("err: {}", err)
                                }
                            }
                        }
                        if event.is_readable() {
                            let sender = sender.clone();
                            let mut buf = [0; 1024];
                                match socket.read_exact(&mut buf) {
                                    Ok(_) => {
                                        tp.spawn(move || {
                                            let s = String::from_utf8(buf.to_vec()).expect("Fail to convert u8 to string");
                                            println!("服务端接收到客户端 {} 消息: {}", addr, s);
                                            let mut buf = s.as_bytes().to_vec();
                                            buf.resize(1024, 0);
                                            sender.send(s.into_bytes()).expect("Fail to send message to sender");
                                        });
                                    },
                                    
                                    Err(ref err) if err.kind() == ErrorKind::WouldBlock => (),
                                    Err(ref err) if err.kind() == ErrorKind::Interrupted => continue,
                                    Err(_) => {
                                        println!("客户端 {} 退出", addr);
                                    }
                                }
                        }
                    }else {
                        println!("[Debug] 没有找到对应的 socket");
                    };
                    sockets.remove(&token);
               }   
                     
           }
        }
    }
}


// You can run this example from the root of the mio repo:
// cargo run --example tcp_server --features="os-poll net"
// use mio::event::Event;
// use mio::net::{TcpListener, TcpStream};
// use mio::{Events, Interest, Poll, Registry, Token};
// use std::collections::HashMap;
// use std::io::{self, Read, Write};
// use std::str::from_utf8;

// // Setup some tokens to allow us to identify which event is for which socket.
// const SERVER: Token = Token(0);

// // Some data we'll send over the connection.
// const DATA: &[u8] = b"Hello world!\n";

// fn main() -> io::Result<()> {
//     // env_logger::init();

//     // Create a poll instance.
//     let mut poll = Poll::new()?;
//     // Create storage for events.
//     let mut events = Events::with_capacity(128);

//     // Setup the TCP server socket.
//     let addr = "127.0.0.1:8080".parse().unwrap();
//     let mut server = TcpListener::bind(addr)?;

//     // Register the server with poll we can receive events for it.
//     poll.registry()
//         .register(&mut server, SERVER, Interest::READABLE)?;

//     // Map of `Token` -> `TcpStream`.
//     let mut connections = HashMap::new();
//     // Unique token for each incoming connection.
//     let mut unique_token = Token(SERVER.0 + 1);

//     println!("You can connect to the server using `nc`:");
//     println!(" $ nc 127.0.0.1 9000");
//     println!("You'll see our welcome message and anything you type will be printed here.");

//     loop {
//         poll.poll(&mut events, None)?;

//         for event in events.iter() {
//             match event.token() {
//                 SERVER => loop {
//                     // Received an event for the TCP server socket, which
//                     // indicates we can accept an connection.
//                     let (mut connection, address) = match server.accept() {
//                         Ok((connection, address)) => (connection, address),
//                         Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
//                             // If we get a `WouldBlock` error we know our
//                             // listener has no more incoming connections queued,
//                             // so we can return to polling and wait for some
//                             // more.
//                             break;
//                         }
//                         Err(e) => {
//                             // If it was any other kind of error, something went
//                             // wrong and we terminate with an error.
//                             return Err(e);
//                         }
//                     };

//                     println!("Accepted connection from: {}", address);

//                     let token = next(&mut unique_token);
//                     poll.registry().register(
//                         &mut connection,
//                         token,
//                         Interest::READABLE.add(Interest::WRITABLE),
//                     )?;

//                     connections.insert(token, connection);
//                 },
//                 token => {
//                     // Maybe received an event for a TCP connection.
//                     let done = if let Some(connection) = connections.get_mut(&token) {
//                         handle_connection_event(poll.registry(), connection, event)?
//                     } else {
//                         // Sporadic events happen, we can safely ignore them.
//                         false
//                     };
//                     if done {
//                         connections.remove(&token);
//                     }
//                 }
//             }
//         }
//     }
// }

// fn next(current: &mut Token) -> Token {
//     let next = current.0;
//     current.0 += 1;
//     Token(next)
// }

// /// Returns `true` if the connection is done.
// fn handle_connection_event(
//     registry: &Registry,
//     connection: &mut TcpStream,
//     event: &Event,
// ) -> io::Result<bool> {
//     if event.is_writable() {
//         // We can (maybe) write to the connection.
//         match connection.write(DATA) {
//             // We want to write the entire `DATA` buffer in a single go. If we
//             // write less we'll return a short write error (same as
//             // `io::Write::write_all` does).
//             Ok(n) if n < DATA.len() => return Err(io::ErrorKind::WriteZero.into()),
//             Ok(_) => {
//                 // After we've written something we'll reregister the connection
//                 // to only respond to readable events.
//                 registry.reregister(connection, event.token(), Interest::READABLE)?
//             }
//             // Would block "errors" are the OS's way of saying that the
//             // connection is not actually ready to perform this I/O operation.
//             Err(ref err) if would_block(err) => {}
//             // Got interrupted (how rude!), we'll try again.
//             Err(ref err) if interrupted(err) => {
//                 return handle_connection_event(registry, connection, event)
//             }
//             // Other errors we'll consider fatal.
//             Err(err) => return Err(err),
//         }
//     }

//     if event.is_readable() {
//         let mut connection_closed = false;
//         let mut received_data = vec![0; 4096];
//         let mut bytes_read = 0;
//         // We can (maybe) read from the connection.
//         loop {
//             match connection.read(&mut received_data[bytes_read..]) {
//                 Ok(0) => {
//                     // Reading 0 bytes means the other side has closed the
//                     // connection or is done writing, then so are we.
//                     connection_closed = true;
//                     break;
//                 }
//                 Ok(n) => {
//                     bytes_read += n;
//                     if bytes_read == received_data.len() {
//                         received_data.resize(received_data.len() + 1024, 0);
//                     }
//                 }
//                 // Would block "errors" are the OS's way of saying that the
//                 // connection is not actually ready to perform this I/O operation.
//                 Err(ref err) if would_block(err) => break,
//                 Err(ref err) if interrupted(err) => continue,
//                 // Other errors we'll consider fatal.
//                 Err(err) => return Err(err),
//             }
//         }

//         if bytes_read != 0 {
//             let received_data = &received_data[..bytes_read];
//             if let Ok(str_buf) = from_utf8(received_data) {
//                 println!("Received data: {}", str_buf.trim_end());
//             } else {
//                 println!("Received (none UTF-8) data: {:?}", received_data);
//             }
//         }

//         if connection_closed {
//             println!("Connection closed");
//             return Ok(true);
//         }
//     }

//     Ok(false)
// }

// fn would_block(err: &io::Error) -> bool {
//     err.kind() == io::ErrorKind::WouldBlock
// }

// fn interrupted(err: &io::Error) -> bool {
//     err.kind() == io::ErrorKind::Interrupted
// }
