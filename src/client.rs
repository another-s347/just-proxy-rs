pub mod message;
pub mod socks;
pub mod connector;

use std::net;
use std::str::FromStr;

use actix::actors::resolver::{Connect, Connector};
use tokio::prelude::*;
use tokio::io::WriteHalf;
use actix::prelude::*;
use actix::io::{FramedWrite, Writer};
use tokio::codec::FramedRead;
use tokio::net::{TcpStream, TcpListener};
use byteorder::ReadBytesExt;
use std::io;
use packet_toolbox_rs::socks5::{message as SocksMessage, codec};
use message as ActorMessage;
use uuid;
use std::collections::HashMap;
use socks::SocksClient;
use connector::ConnectorMessage;

pub struct Server<W>
    where W: AsyncWrite+'static
{
    clients: HashMap<uuid::Uuid, Addr<SocksClient<W>>>,
    writer: FramedWrite<WriteHalf<TcpStream>, ActorMessage::ProxyRequestCodec>,
    //chat: Addr<ChatServer>,
}

/// Make actor from `Server`
impl<W> Actor for Server<W> where W: AsyncWrite+'static
{
    /// Every actor has to provide execution `Context` in which it can run.
    type Context = Context<Self>;
}

#[derive(Message)]
struct TcpConnect(pub TcpStream, pub net::SocketAddr);

impl<W> Handler<message::ProxyRequest> for Server<W> where W: AsyncWrite+'static
{
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyRequest, ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg)
    }
}

impl<W> StreamHandler<message::ProxyResponse, io::Error> for Server<W> where W: AsyncWrite+'static
{
    fn handle(&mut self, item: ActorMessage::ProxyResponse, ctx: &mut Self::Context) {
        let uuid = item.uuid;
        let response = item.response;
        let socks_client = self.clients.get(&uuid).unwrap();
        match response {
            ActorMessage::ProxyTransfer::Data(data) => {
                socks_client.do_send(ActorMessage::ConnectorResponse::Data(data))
            }
            ActorMessage::ProxyTransfer::RequestAddr(_) => {
                panic!();
            }
            ActorMessage::ProxyTransfer::Response(r) => {
                match r {
                    ActorMessage::ProxyResponseType::Succeeded => {
                        //println!("connected in proxy server");
                        socks_client.do_send(ActorMessage::ConnectorResponse::Succeeded)
                    }
                    ActorMessage::ProxyResponseType::ConnectionRefused => {
                        socks_client.do_send(ActorMessage::ConnectorResponse::Failed)
                    }
                    ActorMessage::ProxyResponseType::Timeout => {
                        socks_client.do_send(ActorMessage::ConnectorResponse::Failed)
                    }
                }
            }
        }
    }
}

impl<W> actix::io::WriteHandler<io::Error> for Server<W> where W:AsyncWrite+'static {}

/// Handle stream of TcpStream's
impl<W> Handler<ConnectorMessage<W>> for Server<W> where W: AsyncRead+AsyncWrite+'static
{
    /// this is response for message, which is defined by `ResponseType` trait
    /// in this case we just return unit.
    type Result = ();

    fn handle(&mut self, mut msg: ConnectorMessage<W>, ctx: &mut Context<Self>) {
        let (r, w) = msg.connector.split();
        //println!("add stream");
        let uuid = uuid::Uuid::new_v4();
        let uuid_key = uuid.clone();
        let server_addr = ctx.address();
        let addr = SocksClient::create(move |ctx| {
            SocksClient::add_stream(FramedRead::new(r, codec::Socks5RequestCodec::new()), ctx);
            let writer = actix::io::FramedWrite::new(w, codec::Socks5ResponseCodec, ctx);
            SocksClient { uuid, writer, server_addr }
        });
        self.clients.insert(uuid_key, addr);
    }
}


fn main() {
    actix::System::run(|| {

        // Create server listener
        let addr = net::SocketAddr::from_str("127.0.0.1:12345").unwrap();
        let listener = TcpListener::bind(&addr).unwrap();
        let server_addr = net::SocketAddr::from_str("127.0.0.1:12346").unwrap();
        let server_connect = TcpStream::connect(&server_addr).map(|server_stream| {
            println!("server connected");
            Server::create(move |ctx| {
                ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(|st| {
                    let addr = st.peer_addr().unwrap();
                    ConnectorMessage {
                        connector:st
                    }
                }));
                let (r, w) = server_stream.split();
                Server::add_stream(FramedRead::new(r, ActorMessage::ProxyResponseCodec), ctx);
                Server {
                    clients: HashMap::new(),
                    writer: FramedWrite::new(w, ActorMessage::ProxyRequestCodec, ctx),
                }
            });
        }).map_err(|err| {
            dbg!(err);
        });
        actix::spawn(server_connect);

        println!("Running chat server on 127.0.0.1:12345");
    });
}