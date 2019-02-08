#[macro_use]
extern crate failure;
#[macro_use]
extern crate structopt;

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
use crate::message as ActorMessage;
use uuid;
use std::collections::HashMap;
use crate::connector::ProxyListener;
use std::time::{Instant,Duration};
use structopt::StructOpt;

mod message;
mod connector;
mod ext;
mod opt;

#[derive(Message)]
pub struct ConnectionEstablished<W> where W:AsyncWrite+'static {
    uuid:uuid::Uuid,
    addr:Addr<ProxyEndpointConnection<W>>,
    cost:Duration
}

pub struct ProxyEndpointConnection<W> where W:AsyncWrite+'static {
    uuid: uuid::Uuid,
    client:Addr<ProxyClient<W>>,
    writer:Writer<WriteHalf<TcpStream>,io::Error>
}

#[derive(Message)]
pub struct ProxyConnectionSend(Vec<u8>);

pub struct ProxyServer<W> where W:AsyncWrite+'static {
    clients: Vec<Addr<ProxyClient<W>>>
}

pub struct ProxyClient<W>
    where W:AsyncWrite+'static
{
    writer: FramedWrite<WriteHalf<W>, ActorMessage::ProxyResponseCodec>,
    connections: HashMap<uuid::Uuid, Addr<ProxyEndpointConnection<W>>>,
}

//#[derive(Message)]
//struct TcpConnect(pub TcpStream, pub net::SocketAddr);

impl<W> Actor for ProxyClient<W>
    where W:AsyncWrite+'static
{
    type Context = Context<Self>;

    fn stopped(&mut self, ctx: &mut Self::Context) {
        println!("proxy client actor stopped");
    }
}

impl<W> Actor for ProxyServer<W>
    where W:AsyncWrite+'static
{
    type Context = Context<Self>;
}

impl<W> Actor for ProxyEndpointConnection<W>
    where W:AsyncWrite+'static
{
    type Context = Context<Self>;
}

impl<W> StreamHandler<Vec<u8>,io::Error> for ProxyEndpointConnection<W>
    where W:AsyncWrite+'static
{
    fn handle(&mut self, item: Vec<u8>, ctx: &mut Self::Context) {
        println!("send to client,len {}",item.len());
        self.client.do_send(ActorMessage::ProxyResponse::new(
            self.uuid.clone(),
            ActorMessage::ProxyTransfer::Data(item)
        ))
    }
}

impl<W> Handler<ProxyConnectionSend> for ProxyEndpointConnection<W>
    where W:AsyncWrite+'static
{
    type Result = ();

    fn handle(&mut self, msg: ProxyConnectionSend, ctx: &mut Self::Context) -> Self::Result {
        //println!("send to client,len {}",&msg.0.len());
        self.writer.write(&msg.0);
    }
}

impl<W> actix::io::WriteHandler<io::Error> for ProxyEndpointConnection<W> where W:AsyncWrite+'static {}

impl<W> Handler<ActorMessage::ProxyResponse> for ProxyClient<W>
where W:AsyncWrite+'static
{
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyResponse, ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg)
    }
}

impl<W> Handler<ConnectionEstablished<W>> for ProxyClient<W>
    where W:AsyncWrite+'static
{
    type Result = ();

    fn handle(&mut self, msg: ConnectionEstablished<W>, ctx: &mut Self::Context) -> Self::Result {
        self.connections.insert(msg.uuid.clone(),msg.addr);
        //dbg!(msg.uuid.as_bytes());
        self.writer.write(ActorMessage::ProxyResponse::new(
            msg.uuid,
            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded)
        ))
    }
}

impl<W> StreamHandler<ActorMessage::ProxyRequest, io::Error> for ProxyClient<W>
where W:AsyncWrite+'static
{
    fn handle(&mut self, item: ActorMessage::ProxyRequest, ctx: &mut Self::Context) {
        let uuid=item.uuid;
        let request=item.request;
        match request {
            ActorMessage::ProxyTransfer::RequestAddr(addr)=>{
                let client_addr=ctx.address();
                let client_addr_cloned=client_addr.clone();
                let start=Instant::now();
                Connector::from_registry()
                    .send(Connect::host(addr.clone()))
                    .into_actor(self)
                    .map(move |res, _act, ctx| match res {
                        Ok(stream) => {
                            let cost = Instant::now().duration_since(start);
                            println!("connected addr:{}, cost:{} millis",addr,cost.as_millis());
                            //println!("connected in proxy server");
                            let (r,w)=stream.split();
                            let client_addr_c=client_addr.clone();
                            let conn_addr=actix::Arbiter::start(move|ctx|{
                                ProxyEndpointConnection::add_stream(FramedRead::new(r, ActorMessage::BytesCodec),ctx);
                                let writer=Writer::new(w,ctx);
                                ProxyEndpointConnection {
                                    uuid,
                                    client:client_addr,
                                    writer
                                }
                            });
                            client_addr_c.do_send(ConnectionEstablished{
                                uuid,
                                addr: conn_addr,
                                cost
                            })
                        }
                        Err(err) => {
                            dbg!(err);
                            let cost = Instant::now().duration_since(start);
                            client_addr.do_send(ActorMessage::ProxyResponse::new(
                                uuid,
                                ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Timeout)
                            ));
                            //ctx.stop();
                        }
                    })
                    .map_err(move |err, _act, ctx| {
                        println!("TcpClientActor failed to connected 2: {}", err);
                        dbg!(err);
                        client_addr_cloned.do_send(ActorMessage::ProxyResponse::new(
                            uuid,
                            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::ConnectionRefused)
                        ));
                        ctx.stop();
                    })
                    .wait(ctx);
            }
            ActorMessage::ProxyTransfer::Data(data)=>{
                if let Some(conn)=self.connections.get(&uuid) {
                    conn.do_send(ProxyConnectionSend(data));
                }
                else{
                    panic!()
                }
            }
            ActorMessage::ProxyTransfer::Response(_)=>{
                panic!()
            }
            ActorMessage::ProxyTransfer::Heartbeat=>{
                self.writer.write(ActorMessage::ProxyResponse::new(
                    uuid,
                    ActorMessage::ProxyTransfer::Heartbeat
                ));
            }
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        println!("proxy client stream finished");
        ctx.stop()
    }
}

impl<W> actix::io::WriteHandler<io::Error> for ProxyClient<W> where W:AsyncWrite+'static {}

impl<W> Handler<connector::ConnectorMessage<W>> for ProxyServer<W>
where W:AsyncRead+AsyncWrite+'static+Send
{
    type Result = ();

    fn handle(&mut self, mut msg: connector::ConnectorMessage<W>, ctx: &mut Context<Self>) {
        let (r, w) = msg.connector.split();
        let addr=actix::Arbiter::start(move|ctx|{
            ProxyClient::add_stream(FramedRead::new(r, ActorMessage::ProxyRequestCodec), ctx);
            let writer = actix::io::FramedWrite::new(w, ActorMessage::ProxyResponseCodec, ctx);
            ProxyClient {
                writer,
                connections: HashMap::new(),
            }
        });
        self.clients.push(addr)
    }
}

fn main() {
    let opt:opt::ServerOpt = dbg!(opt::ServerOpt::from_args());

    actix::System::run(move || {
        //let addr = net::SocketAddr::from_str(&format!("{}:{}",opt.socks_host,opt.socks_port)).unwrap();
        // Create server listener
//        let connector=connector::quic::QuicServerConnector::new_dangerous();
        let connector=connector::tcp::TcpConnector{};
        connector.listen(&format!("{}:{}",opt.proxy_host,opt.proxy_port),move|s|{
            ProxyServer::create(|ctx| {
                ctx.add_message_stream(s.map(|st| {
                    connector::ConnectorMessage{
                        connector:st
                    }
                }));
                ProxyServer {
                    clients: Vec::new()
                }
            });
        })
    });
}