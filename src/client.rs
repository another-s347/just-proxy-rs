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

pub mod message;

struct SocksClient {
    pub uuid: uuid::Uuid,
    writer: FramedWrite<WriteHalf<TcpStream>, codec::Socks5ResponseCodec>,
    //peer_stream: Option<Writer<WriteHalf<TcpStream>, io::Error>>,
    server_addr: Addr<Server>,
}

impl Actor for SocksClient {
    type Context = Context<Self>;
}

struct Server {
    clients: HashMap<uuid::Uuid, Addr<SocksClient>>,
    writer: FramedWrite<WriteHalf<TcpStream>, ActorMessage::ProxyRequestCodec>,
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
        self.writer.write(SocksMessage::SocksResponse::Data(item));
    }
}

impl Handler<ActorMessage::ConnectorResponse> for SocksClient {
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ConnectorResponse, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ActorMessage::ConnectorResponse::Succeeded => {
                self.writer.write(SocksMessage::SocksResponse::TargetResponse(SocksMessage::TargetResponse {
                    version: 5,
                    response: SocksMessage::TargetResponseType::Succeeded,
                    reserved: 0,
                    response_type:SocksMessage::SocksRequestType::DOMAIN,
                    address: SocksMessage::SocksAddr::DOMAIN("123".to_string()),
                    port:0,
                }));
            }
            ActorMessage::ConnectorResponse::Failed => {
                self.writer.write(SocksMessage::SocksResponse::TargetResponse(SocksMessage::TargetResponse {
                    version: 5,
                    response: SocksMessage::TargetResponseType::ConnectionRefused,
                    reserved: 0,
                    response_type:SocksMessage::SocksRequestType::DOMAIN,
                    address: SocksMessage::SocksAddr::DOMAIN("".to_string()),
                    port:0,
                }));
            }
            ActorMessage::ConnectorResponse::Data(data)=> {
                self.writer.write(SocksMessage::SocksResponse::Data(data))
            }
        }
    }
}

impl Handler<message::ProxyRequest> for Server {
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyRequest, ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg)
    }
}

impl StreamHandler<message::ProxyResponse, io::Error> for Server {
    fn handle(&mut self, item: ActorMessage::ProxyResponse, ctx: &mut Self::Context) {
        let uuid=item.uuid;
        let response=item.response;
        let socks_client=self.clients.get(&uuid).unwrap();
        match response {
            ActorMessage::ProxyTransfer::Data(data)=>{
                socks_client.do_send(ActorMessage::ConnectorResponse::Data(data))
            }
            ActorMessage::ProxyTransfer::RequestAddr(_)=>{
                panic!();
            }
            ActorMessage::ProxyTransfer::Response(r)=>{
                match r {
                    ActorMessage::ProxyResponseType::Succeeded=>{
                        //println!("connected in proxy server");
                        socks_client.do_send(ActorMessage::ConnectorResponse::Succeeded)
                    }
                    ActorMessage::ProxyResponseType::ConnectionRefused=>{
                        socks_client.do_send(ActorMessage::ConnectorResponse::Failed)
                    }
                    ActorMessage::ProxyResponseType::Timeout=>{
                        socks_client.do_send(ActorMessage::ConnectorResponse::Failed)
                    }
                }
            }
        }
    }
}

impl StreamHandler<SocksMessage::SocksRequest, io::Error> for SocksClient {
    fn handle(&mut self, item: SocksMessage::SocksRequest, ctx: &mut Self::Context) {
        //dbg!(&item);
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
                self.server_addr.do_send(ActorMessage::ProxyRequest::new(
                    self.uuid.clone(),
                    ActorMessage::ProxyTransfer::RequestAddr(s)
                ));
            }
            SocksMessage::SocksRequest::Data(data) => {
                //println!("send to server");
                self.server_addr.do_send(ActorMessage::ProxyRequest::new(
                    self.uuid.clone(),
                    ActorMessage::ProxyTransfer::Data(data)
                ))
            }
        }
    }
}

impl actix::io::WriteHandler<io::Error> for SocksClient {}
impl actix::io::WriteHandler<io::Error> for Server {}

/// Handle stream of TcpStream's
impl Handler<TcpConnect> for Server {
    /// this is response for message, which is defined by `ResponseType` trait
    /// in this case we just return unit.
    type Result = ();

    fn handle(&mut self, mut msg: TcpConnect, ctx: &mut Context<Self>) {
        let (r, w) = msg.0.split();
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
            Server::create(move|ctx| {
                ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(|st| {
                    let addr = st.peer_addr().unwrap();
                    TcpConnect(st, addr)
                }));
                let (r,w)=server_stream.split();
                Server::add_stream(FramedRead::new(r,ActorMessage::ProxyResponseCodec),ctx);
                Server {
                    clients: HashMap::new(),
                    writer:FramedWrite::new(w,ActorMessage::ProxyRequestCodec,ctx)
                }
            });
        }).map_err(|err| {
            dbg!(err);
        });
        actix::spawn(server_connect);

        println!("Running chat server on 127.0.0.1:12345");
    });
}