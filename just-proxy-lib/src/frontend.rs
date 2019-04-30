pub mod agent;

use std::collections::HashMap;
use std::io;
use std::net;
use std::str::FromStr;
use std::time::{Duration, Instant};

use actix::io::FramedWrite;
use actix::prelude::*;
use futures::sync::mpsc::{unbounded, UnboundedSender};
use slog::*;
use slog::Drain;
use structopt::StructOpt;
use tokio::codec::FramedRead;
use tokio::io::WriteHalf;
use tokio::net::TcpListener;
use tokio::prelude::*;
use uuid;

use crate::frontend::agent::{Agent, AgentType};
use crate::message as ActorMessage;
use crate::message::{ProxyRequest, ProxyResponse};
use crate::opt;
use crate::opt::CryptoConfig;
use crate::socks::codec;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Message)]
pub struct Heartbeat;

pub struct FrontEndServer {
    clients: HashMap<u16, Addr<Agent>>,
    hb:Instant,
    hb_index:u32,
    last_hb_instant:Instant,
    logger: Logger,
    writer: Option<UnboundedSender<ProxyRequest>>
}

impl FrontEndServer {
    pub fn new() -> FrontEndServer {
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::CompactFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        let log = slog::Logger::root(drain, o!("FrontEndServer"=>1));
        FrontEndServer {
            clients: HashMap::new(),
            hb: Instant::now(),
            hb_index: 0,
            last_hb_instant: Instant::now(),
            logger: log,
            writer: None
        }
    }

    fn hb(&self, ctx: &mut Context<Self>) {
        let hb_logger=self.logger.clone();
        ctx.run_interval(HEARTBEAT_INTERVAL, move |act, ctx| {
            // check client heartbeats
            let now=Instant::now();
            let hb_duration=now.duration_since(act.hb);
            if hb_duration > Duration::from_secs(10) {
                // heartbeat timed out
                info!(hb_logger,"heartbeat failed, disconnecting!";"duration"=>hb_duration.as_secs());

                // notify chat server
//                ctx.state()
//                    .addr
//                    .do_send(server::Disconnect { id: act.id });
//
//                // stop actor
                //ctx.stop();
//
//                // don't try to send a ping
//                return;
            }
            else {
                ctx.address().do_send(Heartbeat);
            }
        });
    }
}

impl Actor for FrontEndServer {
    type Context = Context<Self>;
}

#[derive(Message)]
pub struct ProtocolServerConnected<S,R>
    where S:AsyncWrite,R:AsyncRead+'static
{
    pub send_stream:S,
    pub recv_stream:R
}

#[derive(Message)]
pub struct FrontendConnected<S,R>
    where S:AsyncWrite,R:AsyncRead
{
    pub send_stream:S,
    pub recv_stream:R,
    pub port:u16,
    pub agentType: AgentType
}

impl<S,R> Handler<ProtocolServerConnected<S,R>> for FrontEndServer
    where S:AsyncWrite+'static,R:AsyncRead+'static
{
    type Result = ();

    async fn handle(&mut self, msg: ProtocolServerConnected<S, R>, ctx: &mut Self::Context) {
        ctx.add_stream(FramedRead::new(msg.recv_stream, ActorMessage::ProxyResponseCodec::new(CryptoConfig::default().convert().unwrap())));
        let (a,b) = unbounded();
        let writer = tokio::codec::FramedWrite::new(msg.send_stream,ActorMessage::ProxyRequestCodec::new(CryptoConfig::default().convert().unwrap()));
        ctx.spawn(b.forward(writer.sink_map_err(|e|())).map(|_|()).into_actor(self));
        self.writer=Some(a);
        self.hb(ctx);
    }
}

impl StreamHandler<ProxyResponse,std::io::Error> for FrontEndServer {
    fn handle(&mut self, item: ProxyResponse, ctx: &mut Self::Context) {
        let uuid = item.uuid;
        let response = item.response;
        if uuid == 0 {
            match response {
                ActorMessage::ProxyTransfer::Heartbeat(i) => {
                    let hb_rtt=Instant::now().duration_since(self.last_hb_instant).as_millis();
                    self.hb=Instant::now();
                    info!(self.logger,"recv heartbeat rtt:{} millis, index {}",hb_rtt,i);
                }
                _=>{
                    panic!()
                }
            }
        }
        else{
            let agent = self.clients.get(&uuid).unwrap();
            match response {
                ActorMessage::ProxyTransfer::Data(data) => {
                    agent.do_send(ActorMessage::ConnectorResponse::Data(data))
                }
                ActorMessage::ProxyTransfer::RequestAddr(_) => {
                    panic!();
                }
                ActorMessage::ProxyTransfer::Response(r) => {
                    match r {
                        ActorMessage::ProxyResponseType::Succeeded => {
                            agent.do_send(ActorMessage::ConnectorResponse::Succeeded)
                        }
                        ActorMessage::ProxyResponseType::ConnectionRefused => {
                            agent.do_send(ActorMessage::ConnectorResponse::Failed)
                        }
                        ActorMessage::ProxyResponseType::Timeout => {
                            agent.do_send(ActorMessage::ConnectorResponse::Failed)
                        }
                        ActorMessage::ProxyResponseType::Abort=>{
                            agent.do_send(ActorMessage::ConnectorResponse::Abort)
                        }
                    }
                }
                ActorMessage::ProxyTransfer::Heartbeat(_) => {
                    panic!()
                }
            }
        }
    }
}

impl<S,R> Handler<FrontendConnected<S,R>> for FrontEndServer
    where S:AsyncWrite+Send+'static,R:AsyncRead+Send+'static
{
    type Result = ();

    fn handle(&mut self, msg: FrontendConnected<S,R>, ctx: &mut Self::Context) -> Self::Result {
        let (r, s) = (msg.recv_stream,msg.send_stream);
        let uuid = msg.port;
        let t = msg.agentType;
        let uuid_key = uuid.clone();
        let server_addr = ctx.address();
        let arbiter = actix::Arbiter::new();
        let logger = self.logger.clone();
        let addr=Agent::start_in_arbiter(&arbiter,move|ctx|{
            let uuid_str=uuid.to_string();
            Agent::new(
                t,
                s,
                r,
                server_addr,
                ctx,
                uuid_key,
                logger
            )
        });
        self.clients.insert(uuid_key, addr);
    }
}

impl Handler<Heartbeat> for FrontEndServer {
    type Result = ();

    fn handle(&mut self, msg: Heartbeat, ctx: &mut Self::Context) -> Self::Result {
        self.last_hb_instant=Instant::now();
        self.hb_index+=1;
        info!(self.logger,"send heartbeat {}",self.hb_index);
        if let Some(writer) = &self.writer {
            writer.unbounded_send(ActorMessage::ProxyRequest::new(
                0,
                ActorMessage::ProxyTransfer::Heartbeat(self.hb_index),
            )).unwrap();
        }
    }
}

impl Handler<ProxyRequest> for FrontEndServer {
    type Result = ();

    fn handle(&mut self, msg: ProxyRequest, ctx: &mut Self::Context) -> Self::Result {
        if let Some(writer) = &self.writer {
            writer.unbounded_send(msg).unwrap();
        }
    }
}