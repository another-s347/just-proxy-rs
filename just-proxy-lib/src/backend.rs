use tokio::prelude::*;
use actix::prelude::*;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::io::{ReadHalf, WriteHalf};
use actix::io::{Writer, FramedWrite};
use std::collections::HashMap;
use actix::actors::resolver;
use crate::message as ActorMessage;
use std::io;
use bytes::Bytes;
use futures::sync::mpsc::{UnboundedSender, unbounded, UnboundedReceiver};
use crate::message::ProxyResponse;

#[derive(Message)]
pub struct ConnectionDead {
    uuid: u16
}

#[derive(Message)]
pub struct ConnectionEstablished {
    uuid: u16,
    writer: WriteHalf<TcpStream>
}

#[derive(Message)]
pub struct ProxyConnectionSend(Bytes);

pub struct ProxyClient
{
    pub write_sender:UnboundedSender<ActorMessage::ProxyResponse>,
    //pub writer: Option<FramedWrite<WriteHalf<W>, ActorMessage::ProxyResponseCodec>>,
    pub connections: HashMap<u16, WriteHalf<TcpStream>>,
    pub logger: slog::Logger,
    pub resolver: Addr<resolver::Resolver>,
}

impl ProxyClient {
    pub fn new(logger:slog::Logger) -> (ProxyClient, UnboundedReceiver<ActorMessage::ProxyResponse>) {
        let (sender,b)=unbounded();
        (ProxyClient {
            write_sender: sender,
            connections: HashMap::new(),
            logger,
            resolver: actix::actors::resolver::Resolver::from_registry()
        },b)
    }
}

impl Actor for ProxyClient
{
    type Context = Context<Self>;

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        println!("proxy client actor stopped");
    }
}

impl Handler<ActorMessage::ProxyResponse> for ProxyClient
{
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyResponse, _ctx: &mut Self::Context) -> Self::Result {
        self.write_sender.unbounded_send(msg).unwrap();
        //self.writer.write(msg)
    }
}

impl Handler<ConnectionDead> for ProxyClient {
    type Result = ();

    fn handle(&mut self, msg: ConnectionDead, ctx: &mut Self::Context) -> Self::Result {
        self.connections.remove(&msg.uuid);
        self.write_sender.unbounded_send(ActorMessage::ProxyResponse::new(
            msg.uuid,
            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Abort),
        )).unwrap();
    }
}

impl Handler<ConnectionEstablished> for ProxyClient
{
    type Result = ();

    fn handle(&mut self, msg: ConnectionEstablished, ctx: &mut Self::Context) -> Self::Result {
        self.connections.insert(msg.uuid.clone(), msg.writer);
        self.write_sender.unbounded_send(ActorMessage::ProxyResponse::new(
            msg.uuid,
            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded),
        )).unwrap();
//        self.writer.write(ActorMessage::ProxyResponse::new(
//            msg.uuid,
//            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded),
//        ))
    }
}

impl StreamHandler<ActorMessage::ProxyRequest, io::Error> for ProxyClient
{
    fn handle(&mut self, item: ActorMessage::ProxyRequest, ctx: &mut Self::Context) {
        let uuid = item.uuid;
        let request = item.request;
        let logger_clone = self.logger.clone();
        match request {
            ActorMessage::ProxyTransfer::RequestAddr(addr) => {
                let client_addr = ctx.address();
                let client_addr_cloned = client_addr.clone();
                let start = Instant::now();
                let resolver_fut=self.resolver.send(resolver::Connect::host(addr.clone())).then(move|result| {
                    match result {
                        Ok(Ok(tcpstream)) => {
                            let cost = Instant::now().duration_since(start);
                            info!(logger_clone, "connected addr:{}, cost:{} millis", addr, cost.as_millis());
                            let (r, w): (ReadHalf<_>, _) = tcpstream.split();
                            let framedreader=tokio::codec::FramedRead::new(r,ActorMessage::BytesCodec);
                            let c2=client_addr.clone();
                            let reader_fut=framedreader.for_each(move|bytes| {
                                client_addr.do_send(ActorMessage::ProxyResponse::new(
                                    uuid.clone(),
                                    ActorMessage::ProxyTransfer::Data(bytes),
                                ));
                                Ok(())
                            }).map_err(move|e|{
                                dbg!(e);
                                c2.do_send(ConnectionDead {
                                    uuid:uuid.clone()
                                });
                            });
                            client_addr_cloned.do_send(ConnectionEstablished {
                                uuid: uuid.clone(),
                                writer: w
                            });
                            actix::Arbiter::spawn(reader_fut);
                        }
                        Ok(Err(err)) => {
                            println!("TcpClientActor failed to connected 1: {}", err);
                            match err {
                                actix::actors::resolver::ResolverError::Timeout=>{
                                    client_addr_cloned.do_send(ActorMessage::ProxyResponse::new(
                                        uuid,
                                        ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Timeout),
                                    ));
                                    return Ok::<_,()>(());
                                }
                                actix::actors::resolver::ResolverError::IoError(io_err)=>{
                                    dbg!(io_err);
                                }
                                other=>{
                                    dbg!(other);
                                }
                            }
                            client_addr_cloned.do_send(ActorMessage::ProxyResponse::new(
                                uuid,
                                ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::ConnectionRefused),
                            ));
                        }
                        Err(err) => {
                            println!("TcpClientActor failed to connected 2: {}", err);
                            dbg!(err);
                            client_addr_cloned.do_send(ActorMessage::ProxyResponse::new(
                                uuid,
                                ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::ConnectionRefused),
                            ));
                        }
                    }
                    Ok::<_, ()>(())
                }).into_actor(self);
                ctx.spawn(resolver_fut);
            }
            ActorMessage::ProxyTransfer::Data(data) => {
                if let Some(conn) = self.connections.get_mut(&uuid) {
                    let r=conn.write(data.as_ref());
                    if r.is_err() {
                        dbg!(r);
                        self.connections.remove(&uuid);
                    }
                } else {
                    panic!()
                }
            }
            ActorMessage::ProxyTransfer::Response(_) => {
                panic!()
            }
            ActorMessage::ProxyTransfer::Heartbeat(index) => {
                info!(self.logger, "echo heartbeat");
                self.write_sender.unbounded_send(ActorMessage::ProxyResponse::new(
                    uuid,
                    ActorMessage::ProxyTransfer::Heartbeat(index),
                )).unwrap();
//                self.writer.write(ActorMessage::ProxyResponse::new(
//                    uuid,
//                    ActorMessage::ProxyTransfer::Heartbeat,
//                ));
            }
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        println!("proxy client stream finished");
        ctx.stop()
    }
}

//impl<W> actix::io::WriteHandler<io::Error> for ProxyClient<W> where W: AsyncWrite + 'static {
//    fn error(&mut self, err: io::Error, ctx: &mut Self::Context) -> Running {
//        dbg!(err);
//        Running::Continue
//    }
//}