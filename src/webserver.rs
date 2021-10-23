use std::net::TcpListener;

use crate::tp::SharedQueueThreadPool;

pub struct WebServer {
    listener: Option<TcpListener>,
    tp: Option<SharedQueueThreadPool>
}

impl WebServer {
    pub fn new<'host>(host: &'host str) -> Self {
        let listener =  TcpListener::bind(host).expect("Fail to bind address");
        Self {
            listener: Some(listener),
            tp: None
        }
    }
}