use tokio::prelude::*;
use tokio::net::{TcpStream,TcpListener};
use std::net;
use std::str::FromStr;
use super::{ProxyConnector,ProxyListener};
use tokio::codec::FramedRead;
use actix::io::FramedWrite;
use crate::component::server::ProxyClient;
use std::collections::HashMap;
use crate::message as ActorMessage;
use actix::prelude::*;

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

impl TcpConnector {
    pub fn run_server(self, addr: &str, logger: slog::Logger) {
        let addr = net::SocketAddr::from_str(addr).unwrap();
        let listener = TcpListener::bind(&addr).unwrap();
        let t=listener.incoming().map_err(|e|{
            dbg!(e);
        }).for_each(move|s|{
            let remote_address = match s.peer_addr() {
                Ok(addr)=>addr.to_string(),
                Err(e)=>e.to_string()
            };
            let proxy_client_logger = logger.new(o!("client"=>remote_address));
            actix::Arbiter::start(move |ctx:&mut actix::Context<ProxyClient>| {
                let (r, w) = s.split();
                let (tx,rx)=futures::sync::mpsc::unbounded();
                let framed_writer = tokio::codec::FramedWrite::new(w,ActorMessage::ProxyResponseCodec);
                let actions = rx.forward(framed_writer.sink_map_err(|e|{
                    dbg!(e);
                })).map_err(|e|{
                    dbg!(e);
                }).map(|_|());
                tokio_current_thread::spawn(actions);
                ctx.add_stream(FramedRead::new(r, ActorMessage::ProxyRequestCodec));
                ProxyClient {
                    write_sender:tx,
                    connections: HashMap::new(),
                    logger:proxy_client_logger,
                    resolver:actix::actors::resolver::Resolver::from_registry()
                }
            });
            Ok(())
        });
        actix::spawn(t);
    }
}

impl ProxyListener<TcpStream> for TcpConnector {
    fn listen<F>(self, addr: &str, f: F) where F: FnOnce(Box<dyn Stream<Item=TcpStream, Error=()>>) + 'static {
        let addr = net::SocketAddr::from_str(addr).unwrap();
        let listener = TcpListener::bind(&addr).unwrap();

        f(Box::new(listener.incoming().map_err(|_|())));
    }
}