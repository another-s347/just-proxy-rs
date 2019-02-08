use tokio::prelude::*;
use std::io;
use tokio::net::{TcpStream,TcpListener};
use std::net;
use std::str::FromStr;
use super::{ProxyConnector,ProxyListener};

pub struct TcpConnector {}

impl ProxyConnector<TcpStream> for TcpConnector {
    fn connect<F>(self,addr:&str,f:F) -> Box<Future<Item=(), Error=()>>
    where F:FnOnce(TcpStream)+'static
    {
        let server_addr = net::SocketAddr::from_str(addr).unwrap();
        Box::new(
        TcpStream::connect(&server_addr).map(|server_stream| {
            f(server_stream);
        }).map_err(|err| {
            dbg!(err);
        }))
    }
}

impl ProxyListener<TcpStream> for TcpConnector {
    fn listen<F>(self, addr: &str, f: F) where F: FnOnce(Box<dyn Stream<Item=TcpStream, Error=()>>) + 'static {
        let addr = net::SocketAddr::from_str(addr).unwrap();
        let listener = TcpListener::bind(&addr).unwrap();

        f(Box::new(listener.incoming().map_err(|_|())));
    }
}