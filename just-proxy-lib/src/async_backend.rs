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
use std::sync::{Arc, Mutex};

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

pub fn process_heartbeat(logger:&slog::Logger, msg:ProxyRequest) -> Option<ProxyResponse> {
    let uuid = msg.uuid;
    match msg.request {
        ActorMessage::ProxyTransfer::Heartbeat(index)=>{
            info!(logger, "echo heartbeat");
            Some(ActorMessage::ProxyResponse::new(
                uuid,
                ActorMessage::ProxyTransfer::Heartbeat(index),
            ))
        }
        _=>{
            None
        }
    }
}

pub struct AsyncProxyClient
{
    pub writer: UnboundedSender<ProxyResponse>,
    pub connections: Arc<Mutex<HashMap<u16, UnboundedSender<Bytes>>>>,
    pub logger: slog::Logger,
    resolver: AsyncResolver,
}

impl AsyncProxyClient {
    pub fn create<T, P>(logger: slog::Logger, sender: T, receiver: P)
        where T: AsyncWrite + Send + 'static, P: AsyncRead + Send + 'static
    {
        let writer = tokio::codec::FramedWrite::new(sender, ActorMessage::ProxyResponseCodec::new(CryptoConfig::default().convert().unwrap()));
        let (s, r) = unbounded();
        tokio::spawn(r.forward(writer.sink_map_err(|e| ())).map(|_| ()));
        let (resolver, background) = AsyncResolver::new(
            ResolverConfig::default(),
            ResolverOpts::default(),
        );
        tokio::spawn(background);
        let mut c = AsyncProxyClient {
            writer: s,
            connections: Arc::new(Mutex::new(HashMap::new())),
            logger,
            resolver,
        };
        let mut reader = FramedRead::new(receiver, ActorMessage::ProxyRequestCodec::new(CryptoConfig::default().convert().unwrap()));
        let task = reader.for_each(move |x| {
            c.handle_message(x);
            Ok(())
        }).map_err(|e| {
            dbg!(e);
        });
        tokio::spawn(task);
    }

    pub fn write(&mut self, msg: ProxyResponse) -> Result<(), futures::sync::mpsc::SendError<ProxyResponse>> {
        self.writer.unbounded_send(msg)
    }

    pub fn handle_message(&mut self, item: ProxyRequest) {
        let uuid = item.uuid;
        let request = item.request;
        let logger_clone = self.logger.clone();
        match request {
            ActorMessage::ProxyTransfer::RequestAddr(addr) => {
                println!("handle");
                let start = Instant::now();
                let addr_par: Vec<&str> = addr.split_terminator(":").collect();
                let host = addr_par[0];
                let port: u16 = addr_par[1].parse().unwrap();
                let task = self.resolver.lookup_ip(host);
                let writer = self.writer.clone();
                let connections = self.connections.clone();
                let task = task.then(move|resolve_result| {
                    let address = resolve_result.unwrap().iter().next().unwrap();
                    let address = SocketAddr::new(address, port);
                    tokio::net::TcpStream::connect(&address)
                }).and_then(move|tcpstream|{
                    println!("connected");
                    let cost = Instant::now().duration_since(start);
                    info!(logger_clone, "connected addr:{}, cost:{} millis", addr, cost.as_millis());
                    let (r, w): (ReadHalf<_>, _) = tcpstream.split();
                    let (channel_s,channel_r) = unbounded();
                    tokio::spawn(channel_r.forward(FramedWrite::new(w,tokio::codec::BytesCodec::new()).sink_map_err(|e|{
                        ()
                    })).map(|_|()));
                    {
                        connections.lock().unwrap().insert(uuid, channel_s);
                    }
                    writer.unbounded_send(ActorMessage::ProxyResponse::new(
                        uuid,
                        ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded),
                    ));
                    let mut framedreader = tokio::codec::FramedRead::new(r, tokio::codec::BytesCodec::new());
                    let task = framedreader.map(move |bytes| {
                        ActorMessage::ProxyResponse::new(
                            uuid.clone(),
                            ActorMessage::ProxyTransfer::Data(bytes.freeze()),
                        )
                    }).map_err(|e| {
                        dbg!(e);
                    }).forward(writer.sink_map_err(|e| {
                        dbg!(e);
                    })).map(|_| ());
                    tokio::spawn(task);
                    Ok(())
                }).map_err(|e| {
                    dbg!(e);
                });
                tokio::spawn(task);
            }
            ActorMessage::ProxyTransfer::Data(data) => {
                if let Some(conn) = self.connections.lock().unwrap().get_mut(&uuid) {
                    conn.unbounded_send(data);
                } else {
                    panic!()
                }
            }
            ActorMessage::ProxyTransfer::Response(_) => {
                panic!()
            }
            ActorMessage::ProxyTransfer::Heartbeat(index) => {
                info!(self.logger, "echo heartbeat");
                self.write(ActorMessage::ProxyResponse::new(
                    uuid,
                    ActorMessage::ProxyTransfer::Heartbeat(index),
                ));
            }
        }
    }
}

//pub fn read_stream<T>(mut client: AsyncProxyClient, recvStream: T)
//    where T: AsyncRead + Send + 'static + Unpin
//{
//    let mut reader = FramedRead::new(recvStream, ActorMessage::ProxyRequestCodec::new(CryptoConfig::default().convert().unwrap()));
//    let task =
//    while let Some(Ok(msg)) = await!(reader.next()) {
//        await!(client.handle_message(msg));
//    }
//}

//pub async fn write_stream(client: &mut AsyncProxyClient, mut reader: FramedRead<ReadHalf<TcpStream>, BytesCodec>) {
//    while let Some(Ok(bytes)) = await!(reader.next()) {
//        await!(client.writer.send_async(ActorMessage::ProxyResponse::new(
//            0,
//            ActorMessage::ProxyTransfer::Data(bytes),
//        )));
//    }
//}

pub struct Connected();