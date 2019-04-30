use actix::prelude::*;
use crate::message as ActorMessage;
use tokio::codec::{FramedRead,FramedWrite};
use crate::socks::codec::{Socks5RequestCodec, Socks5ResponseCodec};
use tokio::prelude::{AsyncRead, AsyncWrite};
use super::FrontEndServer;
use std::time::{Instant, Duration};
use futures::sync::mpsc::{UnboundedSender, unbounded};
use crate::socks::message::{SocksRequest, SocksResponse};
use crate::socks::message as SocksMessage;
use slog::Logger;
use futures::sink::Sink;

pub struct Agent {
    pub inner:AgentInner,
    pub server_addr:Addr<FrontEndServer>,
    pub uuid: u16,
    pub writer: UnboundedSender<SocksResponse>,
//    pub writer: FramedWrite<WriteHalf<TcpStream>, codec::Socks5ResponseCodec>,
    pub logger: Logger,
    pub connect_request_record:Option<Instant>,
    pub connect_rtt:Option<Duration>,
    pub send_bytes:u64,
    pub recv_bytes:u64,
    pub target_address:Option<String>
}

pub enum AgentInner {
    Socks5,
    Http,
    Tap,
    Nop
}

pub enum AgentType {
    Socks5,
    Http,
    Tap,
}

impl Agent {
    pub fn new<S,R>(t:AgentType, send_stream:S, recv_stream:R, frontend:Addr<FrontEndServer>, ctx:&mut Context<Self>, uuid:u16, logger:Logger) -> Agent
        where R:AsyncRead+'static,S:AsyncWrite+'static
    {
        let (x,r) = unbounded();
        let ret = Agent {
            inner:AgentInner::Socks5,
            server_addr: frontend,
            uuid,
            writer: x,
            connect_request_record: None,
            connect_rtt: None,
            send_bytes: 0,
            recv_bytes: 0,
            target_address: None,
            logger
        };
        match t {
            AgentType::Socks5=>{
                let writer = FramedWrite::new(send_stream, Socks5ResponseCodec {});
                ctx.add_stream(FramedRead::new(recv_stream, Socks5RequestCodec::new()));
                ctx.spawn(r.forward(writer.sink_map_err(|_|())).map(|_|()).into_actor(&ret));
            }
            _=>{
                unimplemented!()
            }
        };
        ret
    }
}

impl Actor for Agent {
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

impl Handler<ActorMessage::ConnectorResponse> for Agent {
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ConnectorResponse, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            ActorMessage::ConnectorResponse::Succeeded => {
                self.connect_rtt =Some(Instant::now().duration_since(self.connect_request_record.unwrap()));
                self.writer.unbounded_send(SocksMessage::SocksResponse::TargetResponse(SocksMessage::TargetResponse {
                    version: 5,
                    response: SocksMessage::TargetResponseType::Succeeded,
                    reserved: 0,
                    response_type:SocksMessage::SocksRequestType::DOMAIN,
                    address: SocksMessage::SocksAddr::DOMAIN("123".to_string()),
                    port:0,
                })).unwrap();
            }
            ActorMessage::ConnectorResponse::Failed => {
                self.connect_rtt =Some(Instant::now().duration_since(self.connect_request_record.unwrap()));
                info!(self.logger,"Connect fail";"address"=>self.target_address.clone().unwrap());
                self.writer.unbounded_send(SocksMessage::SocksResponse::TargetResponse(SocksMessage::TargetResponse {
                    version: 5,
                    response: SocksMessage::TargetResponseType::ConnectionRefused,
                    reserved: 0,
                    response_type:SocksMessage::SocksRequestType::DOMAIN,
                    address: SocksMessage::SocksAddr::DOMAIN("".to_string()),
                    port:0,
                })).unwrap();
            }
            ActorMessage::ConnectorResponse::Data(data) => {
                self.recv_bytes+=data.len() as u64;
                self.writer.unbounded_send(SocksMessage::SocksResponse::Data(data)).unwrap();
            }
            ActorMessage::ConnectorResponse::Abort =>{
                ctx.stop()
            }
        };
    }
}

impl StreamHandler<SocksRequest,std::io::Error> for Agent {
    fn handle(&mut self, item: SocksRequest, ctx: &mut Self::Context) {
        match item {
            SocksMessage::SocksRequest::Negotiation(_) => {
                self.writer.unbounded_send(SocksMessage::SocksResponse::Negotiation(SocksMessage::MethodSelectionResponse {
                    version: 5,
                    method: 0,
                })).unwrap();
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
        };
    }
}