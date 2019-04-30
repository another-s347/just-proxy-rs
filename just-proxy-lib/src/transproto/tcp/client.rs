use std::net;
use std::str::FromStr;
use slog::Drain;
use actix::Addr;
use tokio::net::TcpStream;
use tokio::prelude::future::Future;
use tokio::io::{AsyncRead,AsyncWrite};
use crate::frontend::{FrontEndServer, ProtocolServerConnected};

pub fn connect(addr: &str, frontend_server: Addr<FrontEndServer>) {
    let server_addr = net::SocketAddr::from_str(addr).unwrap();
    let connect_future = TcpStream::connect(&server_addr)
        .map_err(|e| {
            dbg!(e);
        })
        .and_then(move |tcpStream| {
            let (recv_stream,send_stream) = tcpStream.split();
            frontend_server.do_send(ProtocolServerConnected {
                send_stream,
                recv_stream,
            });
            Ok(())
        });
    actix::spawn(connect_future);
}

pub fn new_client_logger() -> slog::Logger {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = slog::Logger::root(drain, o!("c"=>1));
    log
}