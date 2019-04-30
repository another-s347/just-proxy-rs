use std::{fs, io, net};
use std::any::Any;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use actix::prelude::*;
use quinn::{Connection, ConnectionDriver, IncomingStreams, ServerConfigBuilder};
use slog::Drain;
use tokio::codec::FramedRead;
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;

use crate::async_backend::{AsyncProxyClient, read_stream};
use crate::backend::ProxyClient;
use crate::message as ActorMessage;
use crate::opt;
use crate::opt::CryptoConfig;

pub async fn run_server(addr: &str, logger: slog::Logger, config: Option<opt::Config>) {
    println!("{}",addr);
    let addr = net::SocketAddr::from_str(addr).unwrap();
    let listener = TcpListener::bind(&addr).unwrap();
    let mut incoming = listener.incoming();
    while let Some(Ok(what)) = await!(incoming.next()) {
        await!(handle_new_client_async(what));
    }
}

pub async fn handle_new_client_async(mut tcpStream: TcpStream) {
    let remote_address = match tcpStream.peer_addr() {
        Ok(addr) => addr.to_string(),
        Err(e) => e.to_string()
    };
    let (recvStream,sendStream) = tcpStream.split();
    let client_logger = new_client_logger(remote_address.clone());
    let mut async_client = AsyncProxyClient::new(client_logger, sendStream);
    tokio::spawn_async(read_stream(async_client, recvStream));
}

pub fn new_client_logger(addr:String)->slog::Logger{
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = slog::Logger::root(drain, o!("client"=>addr));
    log
}