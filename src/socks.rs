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
use super::message as ActorMessage;
use uuid;
use std::collections::HashMap;
use crate::Server;

pub struct SocksClient<W>
where W:AsyncWrite+'static
{
    pub uuid: uuid::Uuid,
    pub writer: FramedWrite<WriteHalf<W>, codec::Socks5ResponseCodec>,
    //peer_stream: Option<Writer<WriteHalf<TcpStream>, io::Error>>,
    pub server_addr: Addr<Server<W>>,
}

impl<W> Actor for SocksClient<W> where W:AsyncWrite+'static {
    type Context = Context<Self>;
}

impl<W> Handler<ActorMessage::ConnectorResponse> for SocksClient<W> where W:AsyncWrite+'static {
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

impl<W> StreamHandler<Vec<u8>, io::Error> for SocksClient<W> where W:AsyncWrite+'static {
    fn handle(&mut self, item: Vec<u8>, ctx: &mut Self::Context) {
        self.writer.write(SocksMessage::SocksResponse::Data(item));
    }
}

impl<W> StreamHandler<SocksMessage::SocksRequest, io::Error> for SocksClient<W> where W:AsyncWrite+'static {
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

impl<W> actix::io::WriteHandler<io::Error> for SocksClient<W> where W:AsyncWrite+'static {}