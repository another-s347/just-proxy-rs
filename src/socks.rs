use tokio::prelude::*;
use tokio::io::WriteHalf;
use actix::prelude::*;
use actix::io::{FramedWrite};
use tokio::net::{TcpStream};
use std::io;
use packet_toolbox_rs::socks5::{message as SocksMessage, codec};
use super::message as ActorMessage;
use uuid;
use crate::Server;
use slog::*;
use std::time::{Instant,Duration};

#[derive(Message)]
pub struct SocksConnectedMessage {
    pub connector:TcpStream
}

pub struct SocksClient<W>
where W:AsyncWrite+'static
{
    pub uuid: uuid::Uuid,
    pub writer: FramedWrite<WriteHalf<TcpStream>, codec::Socks5ResponseCodec>,
    //peer_stream: Option<Writer<WriteHalf<TcpStream>, io::Error>>,
    pub server_addr: Addr<Server<W>>,
    pub logger: Logger,
    pub connect_request_record:Option<Instant>,
    pub connect_rtt:Option<Duration>,
    pub send_bytes:u64,
    pub recv_bytes:u64,
    pub target_address:Option<String>
}

impl<W> SocksClient<W> where W:AsyncWrite+'static {
    pub fn new(uuid:uuid::Uuid,writer:FramedWrite<WriteHalf<TcpStream>, codec::Socks5ResponseCodec>,server_addr:Addr<Server<W>>,logger:Logger)->SocksClient<W> {
        SocksClient {
            uuid,
            writer,
            server_addr,
            logger,
            connect_request_record:None,
            connect_rtt:None,
            send_bytes:0,
            recv_bytes:0,
            target_address:None
        }
    }
}

impl<W> Actor for SocksClient<W> where W:AsyncWrite+'static {
    type Context = Context<Self>;

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        let cc=if let Some(rtt)=self.connect_rtt.clone() {
            format!("{} millis",rtt.as_millis())
        }
        else{
            "unavailable".to_owned()
        };
        let send = self.send_bytes.clone();
        let recv=self.recv_bytes.clone();
        let address=if let Some(addr)=self.target_address.clone() {
            addr
        }
        else {
            "unavailable".to_owned()
        };
        info!(self.logger,"socks client actor stopped";"address"=>address);
        info!(self.logger,"traffic send:{} bytes, recv:{} bytes",send,recv);
        info!(self.logger,"network";"connect rtt"=>cc);
    }
}

impl<W> Handler<ActorMessage::ConnectorResponse> for SocksClient<W> where W:AsyncWrite+'static {
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ConnectorResponse, _ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ActorMessage::ConnectorResponse::Succeeded => {
                self.connect_rtt =Some(Instant::now().duration_since(self.connect_request_record.unwrap()));
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
                self.connect_rtt =Some(Instant::now().duration_since(self.connect_request_record.unwrap()));
                info!(self.logger,"Connect fail";"address"=>self.target_address.clone().unwrap());
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
                self.recv_bytes+=data.len() as u64;
                self.writer.write(SocksMessage::SocksResponse::Data(data))
            }
        }
    }
}

//impl<W> StreamHandler<Vec<u8>, io::Error> for SocksClient<W> where W:AsyncWrite+'static {
//    fn handle(&mut self, item: Vec<u8>, ctx: &mut Self::Context) {
//        print!("???");
//        self.writer.write(SocksMessage::SocksResponse::Data(item));
//    }
//}

impl<W> StreamHandler<SocksMessage::SocksRequest, io::Error> for SocksClient<W> where W:AsyncWrite+'static {
    fn handle(&mut self, item: SocksMessage::SocksRequest, _ctx: &mut Self::Context) {
        match item {
            SocksMessage::SocksRequest::Negotiation(_) => {
                self.writer.write(SocksMessage::SocksResponse::Negotiation(SocksMessage::MethodSelectionResponse {
                    version: 5,
                    method: 0,
                }))
            }
            SocksMessage::SocksRequest::TargetRequest(target_request) => {
                let t: SocksMessage::TargetRequest = target_request;
                let s = t.to_addressstring();
                self.connect_request_record=Some(Instant::now());
                self.target_address=Some(s.clone());
                self.server_addr.do_send(ActorMessage::ProxyRequest::new(
                    self.uuid.clone(),
                    ActorMessage::ProxyTransfer::RequestAddr(s)
                ));
            }
            SocksMessage::SocksRequest::Data(data) => {
                //println!("send to server");
                self.send_bytes+=data.len() as u64;
                self.server_addr.do_send(ActorMessage::ProxyRequest::new(
                    self.uuid.clone(),
                    ActorMessage::ProxyTransfer::Data(data)
                ))
            }
        }
    }
}

impl<W> actix::io::WriteHandler<io::Error> for SocksClient<W> where W:AsyncWrite+'static {}