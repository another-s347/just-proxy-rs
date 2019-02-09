use tokio::prelude::*;
use actix::prelude::*;
use std::time::{Instant, Duration};
use tokio::net::TcpStream;
use tokio::io::{ReadHalf, WriteHalf};
use actix::io::{Writer, FramedWrite};
use std::collections::HashMap;
use tokio::codec::FramedRead;
use actix::actors::resolver;
use crate::message as ActorMessage;
use std::io;

#[allow(dead_code)]
#[derive(Message)]
pub struct ConnectionEstablished<W> where W: AsyncWrite + 'static {
    uuid: uuid::Uuid,
    addr: Addr<ProxyEndpointConnection<W>>,
    cost: Duration,
}

pub struct ProxyEndpointConnection<W> where W: AsyncWrite + 'static {
    uuid: uuid::Uuid,
    client: Addr<ProxyClient<W>>,
    writer: Writer<WriteHalf<TcpStream>, io::Error>,
}

#[derive(Message)]
pub struct ProxyConnectionSend(Vec<u8>);

pub struct ProxyClient<W>
    where W: AsyncWrite + 'static
{
    pub writer: FramedWrite<WriteHalf<W>, ActorMessage::ProxyResponseCodec>,
    pub connections: HashMap<uuid::Uuid, Addr<ProxyEndpointConnection<W>>>,
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

impl<W> Actor for ProxyEndpointConnection<W>
    where W: AsyncWrite + 'static
{
    type Context = Context<Self>;
}

impl<W> StreamHandler<Vec<u8>, io::Error> for ProxyEndpointConnection<W>
    where W: AsyncWrite + 'static
{
    fn handle(&mut self, item: Vec<u8>, _ctx: &mut Self::Context) {
        self.client.do_send(ActorMessage::ProxyResponse::new(
            self.uuid.clone(),
            ActorMessage::ProxyTransfer::Data(item),
        ))
    }
}

impl<W> Handler<ProxyConnectionSend> for ProxyEndpointConnection<W>
    where W: AsyncWrite + 'static
{
    type Result = ();

    fn handle(&mut self, msg: ProxyConnectionSend, _ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(&msg.0);
    }
}

impl<W> actix::io::WriteHandler<io::Error> for ProxyEndpointConnection<W> where W: AsyncWrite + 'static {}

impl<W> Handler<ActorMessage::ProxyResponse> for ProxyClient<W>
    where W: AsyncWrite + 'static
{
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyResponse, _ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg)
    }
}

impl<W> Handler<ConnectionEstablished<W>> for ProxyClient<W>
    where W: AsyncWrite + 'static
{
    type Result = ();

    fn handle(&mut self, msg: ConnectionEstablished<W>, _ctx: &mut Self::Context) -> Self::Result {
        self.connections.insert(msg.uuid.clone(), msg.addr);
        self.writer.write(ActorMessage::ProxyResponse::new(
            msg.uuid,
            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded),
        ))
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
                            let client_addr_c = client_addr.clone();
                            let conn_addr = actix::Arbiter::start(move |ctx| {
                                ProxyEndpointConnection::add_stream(FramedRead::new(r, ActorMessage::BytesCodec), ctx);
                                let writer = Writer::new(w, ctx);
                                ProxyEndpointConnection {
                                    uuid,
                                    client: client_addr,
                                    writer,
                                }
                            });
                            client_addr_c.do_send(ConnectionEstablished {
                                uuid,
                                addr: conn_addr,
                                cost,
                            })
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
                if let Some(conn) = self.connections.get(&uuid) {
                    conn.do_send(ProxyConnectionSend(data));
                } else {
                    panic!()
                }
            }
            ActorMessage::ProxyTransfer::Response(_) => {
                panic!()
            }
            ActorMessage::ProxyTransfer::Heartbeat => {
                info!(self.logger, "echo heartbeat");
                self.writer.write(ActorMessage::ProxyResponse::new(
                    uuid,
                    ActorMessage::ProxyTransfer::Heartbeat,
                ));
            }
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        println!("proxy client stream finished");
        ctx.stop()
    }
}

impl<W> actix::io::WriteHandler<io::Error> for ProxyClient<W> where W: AsyncWrite + 'static {}