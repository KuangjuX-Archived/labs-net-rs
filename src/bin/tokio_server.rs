#![warn(rust_2018_idioms)]

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{ mpsc, Mutex };
use tokio_util::codec::{ Framed, LinesCodec };
use tokio_stream::StreamExt;

use futures::SinkExt;

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;

// const MAX_CHANNEL_SZIE: usize = 1024;

/// 在所有客户端中共享的数据
struct Shared {
    peers: HashMap<SocketAddr, mpsc::UnboundedSender<Vec<u8>>>
}

/// 客户端信息
struct Peer {
    lines: Framed<TcpStream, LinesCodec>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>
}

impl Shared {
    fn new() -> Self {
        Self {
            peers: HashMap::new()
        }
    }

    async fn broadcast(&mut self, sender: SocketAddr, message: &str) {
        for peer in self.peers.iter_mut() {
            let _ = peer.1.send(message.into());
        }
    }
}

impl Peer {
    async fn new(
        state: Arc<Mutex<Shared>>,
        lines: Framed<TcpStream, LinesCodec>
    ) -> Peer {
        let addr = lines.get_ref().peer_addr().unwrap();

        let (tx, rx) = mpsc::unbounded_channel();
        
        state.lock().await.peers.insert(addr, tx);

        Self{
            lines,
            rx
        }
    }
}

#[tokio::main]
async fn main() {
    // 异步绑定
    let addr = "127.0.0.1:8080";
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("Listening on: {}", addr);
    // let mut connect_socks = Arc::new(Mutex::new(VecDeque::new()));
    // let (sender, receiver) = mpsc::unbounded_channel::<Vec<u8>>();
    let state = Arc::new(Mutex::new(Shared::new()));
    loop {
        // 异步监听 socket 连接
        let (socket, addr) = listener.accept().await.unwrap();
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let mut lines = Framed::new(socket, LinesCodec::new());
            lines.send("Please enter your username:").await.unwrap();
            // 注册客户端，并将其加入到共享变量中
            let mut peer = Peer::new(state.clone(), lines).await;
            let mut state_guard = state.lock().await;
            println!("{} joined connection", addr);
            let msg = format!("{} has joined", addr);
            state_guard.broadcast(addr, &msg).await;
            drop(state_guard);
            loop {
                tokio::select! {
                    // 当检测到状态变化则运行
                    Some(msg) = peer.rx.recv() => {
                        // 当从客户端的 channel 中收到消息时将其发送给客户端
                        peer.lines.send(String::from_utf8(msg).unwrap()).await
                                .expect("Failed to send msg");
                    }

                    result = peer.lines.next() => match result {
                        Some(Ok(msg)) => {
                            // 当从客户端收到消息时，将其广播给出了自己的所有客户端
                            let mut state_guard = state.lock().await;
                            state_guard.broadcast(addr, &msg).await;
                            drop(state_guard);
                        },

                        Some(Err(e)) => {
                            println!("err: {}", e);
                        },

                        None => break,
                    }
                }
            }

            // 客户端断开连接
            let mut state_guard = state.lock().await;
            state_guard.peers.remove(&addr);
            let msg = format!("{} has left", addr);
            println!("{}", msg);
            state_guard.broadcast(addr, &msg).await;
            drop(state_guard);
        });
    }
}