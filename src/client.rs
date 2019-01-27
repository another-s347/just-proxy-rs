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

struct SocksClient {
    uuid: uuid::Uuid,
    writer: FramedWrite<WriteHalf<TcpStream>, codec::Socks5ResponseCodec>,
    peer_stream: Option<Writer<WriteHalf<TcpStream>, io::Error>>,
}

impl Actor for SocksClient {
    type Context = Context<Self>;
}

struct Server {
    clients:HashMap<uuid::Uuid,Addr<SocksClient>>
    //chat: Addr<ChatServer>,
}

/// Make actor from `Server`
impl Actor for Server {
    /// Every actor has to provide execution `Context` in which it can run.
    type Context = Context<Self>;
}

#[derive(Message)]
struct TcpConnect(pub TcpStream, pub net::SocketAddr);

impl StreamHandler<Vec<u8>, io::Error> for SocksClient {
    fn handle(&mut self, item: Vec<u8>, ctx: &mut Self::Context) {
        //println!("write to client len:{}",&item.len());
        self.writer.write(SocksMessage::SocksResponse::Data(item));
    }
}

impl Handler<ActorMessage::ConnectorResponse> for SocksClient {
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ConnectorResponse, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ActorMessage::ConnectorResponse::Successed { addr, port, stream } => {
                if self.peer_stream.is_none() {
                    match stream {
                        message::ConnectionWriter::Tcp(tcpstream) => {
                            let (r, w) = tcpstream.split();
                            ctx.add_stream(FramedRead::new(r, message::BytesCodec));
                            self.peer_stream = Some(Writer::new(w, ctx));
                        }
                    }
                    let response_type = match &addr {
                        SocksMessage::SocksAddr::DOMAIN(_) => SocksMessage::SocksRequestType::DOMAIN,
                        SocksMessage::SocksAddr::IPV4(_) => SocksMessage::SocksRequestType::IpV4,
                        SocksMessage::SocksAddr::IPV6(_) => SocksMessage::SocksRequestType::IpV6
                    };
                    self.writer.write(SocksMessage::SocksResponse::TargetResponse(SocksMessage::TargetResponse {
                        version: 5,
                        response: SocksMessage::TargetResponseType::Succeeded,
                        reserved: 0,
                        response_type,
                        address: addr,
                        port,
                    }));
                } else {
                    panic!()
                }
            }
            ActorMessage::ConnectorResponse::Failed { addr, port } => {
                let response_type = match &addr {
                    SocksMessage::SocksAddr::DOMAIN(_) => SocksMessage::SocksRequestType::DOMAIN,
                    SocksMessage::SocksAddr::IPV4(_) => SocksMessage::SocksRequestType::IpV4,
                    SocksMessage::SocksAddr::IPV6(_) => SocksMessage::SocksRequestType::IpV6
                };
                self.writer.write(SocksMessage::SocksResponse::TargetResponse(SocksMessage::TargetResponse {
                    version: 5,
                    response: SocksMessage::TargetResponseType::ConnectionRefused,
                    reserved: 0,
                    response_type,
                    address: addr,
                    port,
                }));
            }
        }
    }
}

impl StreamHandler<SocksMessage::SocksRequest, io::Error> for SocksClient {
    fn handle(&mut self, item: SocksMessage::SocksRequest, ctx: &mut Self::Context) {
        match item {
            SocksMessage::SocksRequest::Negotiation(nego) => {
                self.writer.write(SocksMessage::SocksResponse::Negotiation(SocksMessage::MethodSelectionResponse {
                    version: 5,
                    method: 0,
                }))
            }
            SocksMessage::SocksRequest::TargetRequest(target_request) => {
                let t: SocksMessage::TargetRequest = target_request;
                let t2 = t.clone();
                let s = (t.to_addressstring());
                let addr = ctx.address();
                let addr2 = addr.clone();
                Connector::from_registry()
                    .send(Connect::host(s))
                    .into_actor(self)
                    .map(move |res, _act, ctx| match res {
                        Ok(stream) => {
                            addr.do_send(ActorMessage::ConnectorResponse::Successed {
                                addr: t.address,
                                port: t.port,
                                stream: message::ConnectionWriter::Tcp(stream),
                            });
                        }
                        Err(err) => {
                            dbg!(err);
                            addr.do_send(ActorMessage::ConnectorResponse::Failed {
                                addr: t.address,
                                port: t.port,
                            });
                            ctx.stop();
                        }
                    })
                    .map_err(move |err, _act, ctx| {
                        println!("TcpClientActor failed to connected 2: {}", err);
                        dbg!(err);
                        addr2.do_send(ActorMessage::ConnectorResponse::Failed {
                            addr: t2.address,
                            port: t2.port,
                        });
                        ctx.stop();
                    })
                    .wait(ctx);
            }
            SocksMessage::SocksRequest::Data(data) => {
//                let try_string = String::from_utf8(data);
//                dbg!(try_string);
                if let Some(writer) = &mut self.peer_stream {
                    //println!("send to peer:len {}",&data.len());
                    writer.write(&data);
                }
            }
        }
    }
}

impl actix::io::WriteHandler<io::Error> for SocksClient {}

/// Handle stream of TcpStream's
impl Handler<TcpConnect> for Server {
    /// this is response for message, which is defined by `ResponseType` trait
    /// in this case we just return unit.
    type Result = ();

    fn handle(&mut self, mut msg: TcpConnect, _: &mut Context<Self>) {
        let (r, w) = msg.0.split();
        //println!("add stream");
        let uuid=uuid::Uuid::new_v4();
        let uuid_key=uuid.clone();
        let addr=SocksClient::create(move |ctx| {
            SocksClient::add_stream(FramedRead::new(r, codec::Socks5RequestCodec::new()), ctx);
            let writer = actix::io::FramedWrite::new(w, codec::Socks5ResponseCodec, ctx);
            SocksClient { uuid, writer, peer_stream: None }
        });
        self.clients.insert(uuid_key,addr);
    }
}


fn main() {
    actix::System::run(|| {

        // Create server listener
        let addr = net::SocketAddr::from_str("127.0.0.1:12345").unwrap();
        let listener = TcpListener::bind(&addr).unwrap();
        let server_addr = net::SocketAddr::from_str("127.0.0.1:12346").unwrap();
//        let server_connect=TcpStream::connect(&server_addr).map(|s|{
//
//        }).map_err(|err|{
//            dbg!(err)
//        });

        Server::create(|ctx| {
            ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(|st| {
                let addr = st.peer_addr().unwrap();
                TcpConnect(st, addr)
            }));
            ctx.add_message_stream(TcpStream::connect(&server_addr).map_err(|err|{
                println!("server connect refused");
            }).map(|s|{

            }));
            Server {
                clients:HashMap::new()
            }
        });

        println!("Running chat server on 127.0.0.1:12345");
    });
}