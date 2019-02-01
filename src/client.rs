#[macro_use]
extern crate failure;

pub mod message;
pub mod socks;
pub mod connector;
pub mod ext;

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
use quinn::BiStream;
use packet_toolbox_rs::socks5::{message as SocksMessage, codec};
use message as ActorMessage;
use uuid;
use std::collections::HashMap;
use socks::SocksClient;
use socks::SocksConnectedMessage;
use connector::ProxyConnector;
use std::time::{Duration, Instant};
use uuid::prelude::*;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

pub struct Server<W>
    where W: AsyncWrite + 'static
{
    clients: HashMap<uuid::Uuid, Addr<SocksClient<W>>>,
    writer: FramedWrite<WriteHalf<W>, ActorMessage::ProxyRequestCodec>,
    //chat: Addr<ChatServer>,
    hb:Instant
}

/// Make actor from `Server`
impl<W> Actor for Server<W> where W: AsyncWrite + 'static
{
    /// Every actor has to provide execution `Context` in which it can run.
    type Context = Context<Self>;
}

impl<W> Server<W> where W: AsyncWrite + 'static {
    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > Duration::from_secs(20) {
                // heartbeat timed out
                println!("heartbeat failed, disconnecting!");

                // notify chat server
//                ctx.state()
//                    .addr
//                    .do_send(server::Disconnect { id: act.id });
//
//                // stop actor
//                ctx.stop();
//
//                // don't try to send a ping
//                return;
            }

            ctx.address().do_send(Heartbeat);
        });
    }
}

#[derive(Message)]
pub struct Heartbeat;

impl<W> Handler<message::ProxyRequest> for Server<W> where W: AsyncWrite + 'static
{
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyRequest, ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg)
    }
}

impl<W> StreamHandler<message::ProxyResponse, io::Error> for Server<W> where W: AsyncWrite + 'static
{
    fn handle(&mut self, item: ActorMessage::ProxyResponse, ctx: &mut Self::Context) {
        let uuid = item.uuid;
        let response = item.response;
        if uuid.is_nil() {
            match response {
                ActorMessage::ProxyTransfer::Heartbeat => {
                    self.hb=Instant::now();
                    println!("recv heartbeat");
                }
                _=>{
                    panic!()
                }
            }
        }
        else{
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
                ActorMessage::ProxyTransfer::Heartbeat => {
                    panic!()
                }
            }
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        println!("server disconnected");
        ctx.stop()
    }
}

impl<W> Handler<Heartbeat> for Server<W> where W:AsyncWrite+'static {
    type Result = ();

    fn handle(&mut self, msg: Heartbeat, ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(ActorMessage::ProxyRequest::new(
            uuid::Uuid::nil(),
            ActorMessage::ProxyTransfer::Heartbeat
        ))
    }
}

impl<W> actix::io::WriteHandler<io::Error> for Server<W> where W: AsyncWrite + 'static {}

/// Handle stream of TcpStream's
impl<W> Handler<SocksConnectedMessage> for Server<W>
    where W: AsyncWrite + 'static
{
    /// this is response for message, which is defined by `ResponseType` trait
    /// in this case we just return unit.
    type Result = ();

    fn handle(&mut self, mut msg: SocksConnectedMessage, ctx: &mut Context<Self>) {
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
        let connector = connector::quic::QuicClientConnector::new_dangerous();
//        let connector=connector::tcp::TcpConnector{};
        let f = connector.connect("127.0.0.1:12346", move |stream| {
            println!("server connected");
            Server::create(move |ctx| {
                ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(|st| {
                    let addr = st.peer_addr().unwrap();
                    SocksConnectedMessage {
                        connector: st
                    }
                }));
                let (r, w) = stream.split();
                Server::add_stream(FramedRead::new(r, ActorMessage::ProxyResponseCodec), ctx);
                let writer: FramedWrite<WriteHalf<_>, ActorMessage::ProxyRequestCodec> = FramedWrite::new(w, ActorMessage::ProxyRequestCodec, ctx);
                let s=Server {
                    clients: HashMap::new(),
                    writer,
                    hb:Instant::now()
                };
                s.hb(ctx);
                s
            });
        });
        //let server_addr = net::SocketAddr::from_str().unwrap();
//        let server_connect = TcpStream::connect(&server_addr).map(|server_stream| {
//        }).map_err(|err| {
//            dbg!(err);
//        });
        actix::spawn(f);

        println!("Running chat server on 127.0.0.1:12345");
    });
}