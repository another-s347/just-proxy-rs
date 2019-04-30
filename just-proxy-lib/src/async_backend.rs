use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::time::Instant;

use actix::actors::resolver;
use actix::prelude::*;
use bytes::Bytes;
use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use quinn::{RecvStream, SendStream};
use tokio::codec::{FramedRead, FramedWrite};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::prelude::*;
use trust_dns_resolver::{AsyncResolver, Resolver};
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::error::ResolveError;
use trust_dns_resolver::lookup_ip::LookupIp;

use crate::message as ActorMessage;
use crate::message::{BytesCodec, ProxyRequest, ProxyResponse, ProxyResponseCodec};
use crate::opt::CryptoConfig;

#[derive(Message)]
pub struct ConnectionDead {
    uuid: u16
}

#[derive(Message)]
pub struct ConnectionEstablished {
    uuid: u16,
    writer: WriteHalf<TcpStream>,
}

#[derive(Message)]
pub struct ProxyConnectionSend(Bytes);

pub struct AsyncProxyClient
{
    pub writer: UnboundedSender<ProxyResponse>,
    pub connections: HashMap<u16, WriteHalf<TcpStream>>,
    pub logger: slog::Logger,
    resolver: AsyncResolver,
}

impl AsyncProxyClient {
    pub fn new<T>(logger: slog::Logger, sender: T) -> AsyncProxyClient
        where T: AsyncWrite + Send + 'static
    {
        let writer = tokio::codec::FramedWrite::new(sender, ActorMessage::ProxyResponseCodec::new(CryptoConfig::default().convert().unwrap()));
        let (s, r) = unbounded();
        tokio::spawn(r.forward(writer.sink_map_err(|e| ())).map(|_| ()));
        let (resolver, background) = AsyncResolver::new(
            ResolverConfig::default(),
            ResolverOpts::default(),
        );
        tokio::spawn(background);
        AsyncProxyClient {
            writer: s,
            connections: HashMap::new(),
            logger,
            resolver,
        }
    }

    pub async fn write(&mut self, msg: ProxyResponse) -> Result<(), futures::sync::mpsc::SendError<ProxyResponse>> {
        await!(self.writer.send_async(msg))
    }

    pub async fn handle_message(&mut self, item: ProxyRequest) {
        let uuid = item.uuid;
        let request = item.request;
        let logger_clone = self.logger.clone();
        match request {
            ActorMessage::ProxyTransfer::RequestAddr(addr) => {
                println!("handle");
                let resolver = self.resolver.clone();

                tokio::spawn_async(async move {
                    let start = Instant::now();
                    let addr_par: Vec<&str> = addr.split_terminator(":").collect();
                    let host = addr_par[0];
                    let port: u16 = addr_par[1].parse().unwrap();
                    let resolve_result: Result<LookupIp, ResolveError> = tokio::await!(resolver.lookup_ip(host));
                    println!("look up done");
                    let address = resolve_result.unwrap().iter().next().unwrap();
                    let address = SocketAddr::new(address, port);
                    println!("connect");
                    let tcpstream: TcpStream = tokio::await!(tokio::net::TcpStream::connect(&address)).unwrap();
                    println!("connected");
                    let cost = Instant::now().duration_since(start);
                    info!(logger_clone, "connected addr:{}, cost:{} millis", addr, cost.as_millis());
                    let (r, w): (ReadHalf<_>, _) = tcpstream.split();
                    self.connections.insert(uuid, w);
                    tokio::await!(self.write(ActorMessage::ProxyResponse::new(
                        uuid,
                        ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded),
                    )));
                    let mut framedreader = tokio::codec::FramedRead::new(r, ActorMessage::BytesCodec);
                    let mut writer = self.writer.clone();
                    let task = framedreader.map(move |bytes| {
                        ActorMessage::ProxyResponse::new(
                            uuid.clone(),
                            ActorMessage::ProxyTransfer::Data(bytes),
                        )
                    }).map_err(|e| {
                        dbg!(e);
                    }).forward(writer.sink_map_err(|e| {
                        dbg!(e);
                    })).map(|_| ());
                    tokio::spawn(task);
                });
            }
            ActorMessage::ProxyTransfer::Data(data) => {
                println!("handle data");
                if let Some(conn) = self.connections.get_mut(&uuid) {
                    let r = await!(conn.write_all_async(data.as_ref()));
                    if r.is_err() {
                        dbg!(r);
                        //self.connections.remove(&uuid);
                    }
                    println!("send done");
                } else {
                    panic!()
                }
            }
            ActorMessage::ProxyTransfer::Response(_) => {
                panic!()
            }
            ActorMessage::ProxyTransfer::Heartbeat(index) => {
                info!(self.logger, "echo heartbeat");
                await!(self.write(ActorMessage::ProxyResponse::new(
                    uuid,
                    ActorMessage::ProxyTransfer::Heartbeat(index),
                )));
            }
        }
    }
}

pub async fn read_stream<T>(mut client: AsyncProxyClient, recvStream: T)
    where T: AsyncRead + Send + 'static + Unpin
{
    let mut reader = FramedRead::new(recvStream, ActorMessage::ProxyRequestCodec::new(CryptoConfig::default().convert().unwrap()));
    while let Some(Ok(msg)) = await!(reader.next()) {
        await!(client.handle_message(msg));
    }
}

pub async fn write_stream(client: &mut AsyncProxyClient, mut reader: FramedRead<ReadHalf<TcpStream>, BytesCodec>) {
    while let Some(Ok(bytes)) = await!(reader.next()) {
        await!(client.writer.send_async(ActorMessage::ProxyResponse::new(
            0,
            ActorMessage::ProxyTransfer::Data(bytes),
        )));
    }
}

//impl Actor for ProxyClient
//{
//    type Context = Context<Self>;
//
//    fn stopped(&mut self, _ctx: &mut Self::Context) {
//        println!("proxy client actor stopped");
//    }
//}
//
//impl Handler<ActorMessage::ProxyResponse> for ProxyClient
//{
//    type Result = ();
//
//    fn handle(&mut self, msg: ActorMessage::ProxyResponse, _ctx: &mut Self::Context) -> Self::Result {
//        self.write_sender.unbounded_send(msg).unwrap();
//        //self.writer.write(msg)
//    }
//}
//
//impl Handler<ConnectionDead> for ProxyClient {
//    type Result = ();
//
//    fn handle(&mut self, msg: ConnectionDead, ctx: &mut Self::Context) -> Self::Result {
//        self.connections.remove(&msg.uuid);
//        self.write_sender.unbounded_send(ActorMessage::ProxyResponse::new(
//            msg.uuid,
//            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Abort),
//        )).unwrap();
//    }
//}
//
//impl Handler<ConnectionEstablished> for ProxyClient
//{
//    type Result = ();
//
//    fn handle(&mut self, msg: ConnectionEstablished, ctx: &mut Self::Context) -> Self::Result {
//        self.connections.insert(msg.uuid.clone(), msg.writer);
//        self.write_sender.unbounded_send(ActorMessage::ProxyResponse::new(
//            msg.uuid,
//            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded),
//        )).unwrap();
////        self.writer.write(ActorMessage::ProxyResponse::new(
////            msg.uuid,
////            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded),
////        ))
//    }
//}
//
//impl StreamHandler<ActorMessage::ProxyRequest, io::Error> for ProxyClient
//{
//    fn handle(&mut self, item: ActorMessage::ProxyRequest, ctx: &mut Self::Context) {
//        let uuid = item.uuid;
//        let request = item.request;
//        let logger_clone = self.logger.clone();
//        match request {
//            ActorMessage::ProxyTransfer::RequestAddr(addr) => {
//                let client_addr = ctx.address();
//                let client_addr_cloned = client_addr.clone();
//                let start = Instant::now();
//                let resolver_fut=self.resolver.send(resolver::Connect::host(addr.clone())).then(move|result| {
//                    match result {
//                        Ok(Ok(tcpstream)) => {
//                            let cost = Instant::now().duration_since(start);
//                            info!(logger_clone, "connected addr:{}, cost:{} millis", addr, cost.as_millis());
//                            let (r, w): (ReadHalf<_>, _) = tcpstream.split();
//                            let framedreader=tokio::codec::FramedRead::new(r,ActorMessage::BytesCodec);
//                            let c2=client_addr.clone();
//                            let reader_fut=framedreader.for_each(move|bytes| {
//                                client_addr.do_send(ActorMessage::ProxyResponse::new(
//                                    uuid.clone(),
//                                    ActorMessage::ProxyTransfer::Data(bytes),
//                                ));
//                                Ok(())
//                            }).map_err(move|e|{
//                                dbg!(e);
//                                c2.do_send(ConnectionDead {
//                                    uuid:uuid.clone()
//                                });
//                            });
//                            client_addr_cloned.do_send(ConnectionEstablished {
//                                uuid: uuid.clone(),
//                                writer: w
//                            });
//                            actix::Arbiter::spawn(reader_fut);
//                        }
//                        Ok(Err(err)) => {
//                            println!("TcpClientActor failed to connected 1: {}", err);
//                            match err {
//                                actix::actors::resolver::ResolverError::Timeout=>{
//                                    client_addr_cloned.do_send(ActorMessage::ProxyResponse::new(
//                                        uuid,
//                                        ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Timeout),
//                                    ));
//                                    return Ok::<_,()>(());
//                                }
//                                actix::actors::resolver::ResolverError::IoError(io_err)=>{
//                                    dbg!(io_err);
//                                }
//                                other=>{
//                                    dbg!(other);
//                                }
//                            }
//                            client_addr_cloned.do_send(ActorMessage::ProxyResponse::new(
//                                uuid,
//                                ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::ConnectionRefused),
//                            ));
//                        }
//                        Err(err) => {
//                            println!("TcpClientActor failed to connected 2: {}", err);
//                            dbg!(err);
//                            client_addr_cloned.do_send(ActorMessage::ProxyResponse::new(
//                                uuid,
//                                ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::ConnectionRefused),
//                            ));
//                        }
//                    }
//                    Ok::<_, ()>(())
//                }).into_actor(self);
//                ctx.spawn(resolver_fut);
//            }
//            ActorMessage::ProxyTransfer::Data(data) => {
//                if let Some(conn) = self.connections.get_mut(&uuid) {
//                    let r=conn.write(data.as_ref());
//                    if r.is_err() {
//                        dbg!(r);
//                        self.connections.remove(&uuid);
//                    }
//                } else {
//                    panic!()
//                }
//            }
//            ActorMessage::ProxyTransfer::Response(_) => {
//                panic!()
//            }
//            ActorMessage::ProxyTransfer::Heartbeat(index) => {
//                info!(self.logger, "echo heartbeat");
//                self.write_sender.unbounded_send(ActorMessage::ProxyResponse::new(
//                    uuid,
//                    ActorMessage::ProxyTransfer::Heartbeat(index),
//                )).unwrap();
////                self.writer.write(ActorMessage::ProxyResponse::new(
////                    uuid,
////                    ActorMessage::ProxyTransfer::Heartbeat,
////                ));
//            }
//        }
//    }
//
//    fn finished(&mut self, ctx: &mut Self::Context) {
//        println!("proxy client stream finished");
//        ctx.stop()
//    }
//}

//impl<W> actix::io::WriteHandler<io::Error> for ProxyClient<W> where W: AsyncWrite + 'static {
//    fn error(&mut self, err: io::Error, ctx: &mut Self::Context) -> Running {
//        dbg!(err);
//        Running::Continue
//    }
//}

pub struct Connected();