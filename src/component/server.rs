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
use futures::sync::mpsc::UnboundedSender;

#[allow(dead_code)]
#[derive(Message)]
pub struct ConnectionEstablished {
    uuid: uuid::Uuid,
    writer: WriteHalf<TcpStream>
}

#[derive(Message)]
pub struct ProxyConnectionSend(Bytes);

pub struct ProxyClient<W>
    where W: AsyncWrite + 'static
{
    pub write_sender:UnboundedSender<ActorMessage::ProxyResponse>,
    pub writer: Option<FramedWrite<WriteHalf<W>, ActorMessage::ProxyResponseCodec>>,
    pub connections: HashMap<uuid::Uuid, WriteHalf<TcpStream>>,
    pub logger: slog::Logger,
    pub resolver: Addr<resolver::Resolver>,
}

impl<W> Actor for ProxyClient<W>
    where W: AsyncWrite + 'static
{
    type Context = Context<Self>;

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        println!("proxy client actor stopped");
    }
}

impl<W> Handler<ActorMessage::ProxyResponse> for ProxyClient<W>
    where W: AsyncWrite + 'static
{
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyResponse, _ctx: &mut Self::Context) -> Self::Result {
        self.write_sender.unbounded_send(msg).unwrap();
        //self.writer.write(msg)
    }
}

impl<W> Handler<ConnectionEstablished> for ProxyClient<W>
    where W:AsyncWrite+'static
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

impl<W> StreamHandler<ActorMessage::ProxyRequest, io::Error> for ProxyClient<W>
    where W: AsyncWrite + 'static
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
                            let reader_fut=framedreader.for_each(move|bytes| {
                                client_addr.do_send(ActorMessage::ProxyResponse::new(
                                    uuid.clone(),
                                    ActorMessage::ProxyTransfer::Data(bytes),
                                ));
                                Ok(())
                            }).map_err(|e|{
                                dbg!(e);
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
                    }
                } else {
                    panic!()
                }
            }
            ActorMessage::ProxyTransfer::Response(_) => {
                panic!()
            }
            ActorMessage::ProxyTransfer::Heartbeat => {
                info!(self.logger, "echo heartbeat");
                self.write_sender.unbounded_send(ActorMessage::ProxyResponse::new(
                    uuid,
                    ActorMessage::ProxyTransfer::Heartbeat,
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