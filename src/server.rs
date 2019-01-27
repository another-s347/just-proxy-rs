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
use crate::message as ActorMessage;
use uuid;
use std::collections::HashMap;

mod message;

#[derive(Message)]
pub struct ConnectionEstablished {
    uuid:uuid::Uuid,
    addr:Addr<ProxyEndpointConnection>
}

pub struct ProxyEndpointConnection {
    uuid: uuid::Uuid,
    client:Addr<ProxyClient>,
    writer:Writer<WriteHalf<TcpStream>,io::Error>
}

#[derive(Message)]
pub struct ProxyConnectionSend(Vec<u8>);

pub struct ProxyServer {
    clients: Vec<Addr<ProxyClient>>
}

pub struct ProxyClient {
    writer: FramedWrite<WriteHalf<TcpStream>, ActorMessage::ProxyResponseCodec>,
    connections: HashMap<uuid::Uuid, Addr<ProxyEndpointConnection>>,
}

#[derive(Message)]
struct TcpConnect(pub TcpStream, pub net::SocketAddr);

impl Actor for ProxyClient {
    type Context = Context<Self>;
}

impl Actor for ProxyServer {
    type Context = Context<Self>;
}

impl Actor for ProxyEndpointConnection {
    type Context = Context<Self>;
}

impl StreamHandler<Vec<u8>,io::Error> for ProxyEndpointConnection {
    fn handle(&mut self, item: Vec<u8>, ctx: &mut Self::Context) {
        //println!("send to client,len {}",item.len());
        self.client.do_send(ActorMessage::ProxyResponse::new(
            self.uuid.clone(),
            ActorMessage::ProxyTransfer::Data(item)
        ))
    }
}

impl Handler<ProxyConnectionSend> for ProxyEndpointConnection {
    type Result = ();

    fn handle(&mut self, msg: ProxyConnectionSend, ctx: &mut Self::Context) -> Self::Result {
        //println!("send to client,len {}",&msg.0.len());
        self.writer.write(&msg.0);
    }
}

impl actix::io::WriteHandler<io::Error> for ProxyEndpointConnection {}

impl Handler<ActorMessage::ProxyResponse> for ProxyClient {
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyResponse, ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg)
    }
}

impl Handler<ConnectionEstablished> for ProxyClient{
    type Result = ();

    fn handle(&mut self, msg: ConnectionEstablished, ctx: &mut Self::Context) -> Self::Result {
        self.connections.insert(msg.uuid.clone(),msg.addr);
        //dbg!(msg.uuid.as_bytes());
        self.writer.write(ActorMessage::ProxyResponse::new(
            msg.uuid,
            ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Succeeded)
        ))
    }
}

impl StreamHandler<ActorMessage::ProxyRequest, io::Error> for ProxyClient {
    fn handle(&mut self, item: ActorMessage::ProxyRequest, ctx: &mut Self::Context) {
        let uuid=item.uuid;
        let request=item.request;
        match request {
            ActorMessage::ProxyTransfer::RequestAddr(addr)=>{
                let client_addr=ctx.address();
                let client_addr_cloned=client_addr.clone();
                //dbg!(&addr);
                Connector::from_registry()
                    .send(Connect::host(addr))
                    .into_actor(self)
                    .map(move |res, _act, ctx| match res {
                        Ok(stream) => {
                            //println!("connected in proxy server");
                            let (r,w)=stream.split();
                            let client_addr_c=client_addr.clone();
                            let conn_addr=ProxyEndpointConnection::create(move|ctx|{
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
                                addr: conn_addr
                            })
                        }
                        Err(err) => {
                            dbg!(err);
                            client_addr.do_send(ActorMessage::ProxyResponse::new(
                                uuid,
                                ActorMessage::ProxyTransfer::Response(ActorMessage::ProxyResponseType::Timeout)
                            ));
                            ctx.stop();
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
        }
    }
}

impl actix::io::WriteHandler<io::Error> for ProxyClient {}

impl Handler<TcpConnect> for ProxyServer {
    type Result = ();

    fn handle(&mut self, mut msg: TcpConnect, ctx: &mut Context<Self>) {
        let (r, w) = msg.0.split();
        let addr = ProxyClient::create(move |ctx| {
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
    actix::System::run(|| {

        // Create server listener
        let addr = net::SocketAddr::from_str("127.0.0.1:12346").unwrap();
        let listener = TcpListener::bind(&addr).unwrap();

        ProxyServer::create(|ctx| {
            ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(|st| {
                let addr = st.peer_addr().unwrap();
                TcpConnect(st, addr)
            }));
            ProxyServer {
                clients: Vec::new()
            }
        });

        println!("Running chat server on 127.0.0.1:12346");
    });
}