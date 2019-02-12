#[macro_use]
extern crate failure;
extern crate structopt;
#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;

pub mod message;
pub mod socks;
pub mod connector;
pub mod ext;
pub mod opt;
pub mod component;

use std::net;
use std::str::FromStr;
use tokio::prelude::*;
use tokio::io::WriteHalf;
use actix::prelude::*;
use actix::io::{FramedWrite};
use tokio::codec::FramedRead;
use tokio::net::{TcpListener};
use std::io;
use packet_toolbox_rs::socks5::codec;
use message as ActorMessage;
use uuid;
use std::collections::HashMap;
use socks::SocksClient;
use socks::SocksConnectedMessage;
use connector::ProxyConnector;
use std::time::{Duration, Instant};
use structopt::StructOpt;
use slog::Drain;
use slog::*;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

pub struct Server<W>
    where W: AsyncWrite + 'static
{
    clients: HashMap<uuid::Uuid, Addr<SocksClient<W>>>,
    writer: FramedWrite<WriteHalf<W>, ActorMessage::ProxyRequestCodec>,
    hb:Instant,
    hb_index:u32,
    logger: Logger,
    last_hb_instant:Instant,
    crypto_config: opt::_CryptoConfig
}

/// Make actor from `Server`
impl<W> Actor for Server<W> where W: AsyncWrite + 'static
{
    /// Every actor has to provide execution `Context` in which it can run.
    type Context = Context<Self>;
}

impl<W> Server<W> where W: AsyncWrite + 'static {
    fn hb(&self, ctx: &mut Context<Self>) {
        let hb_logger=self.logger.clone();
        ctx.run_interval(HEARTBEAT_INTERVAL, move |act, ctx| {
            // check client heartbeats
            let now=Instant::now();
            let hb_duration=now.duration_since(act.hb);
            if hb_duration > Duration::from_secs(15) {
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

#[derive(Message)]
pub struct Heartbeat;

impl<W> Handler<message::ProxyRequest> for Server<W> where W: AsyncWrite + 'static
{
    type Result = ();

    fn handle(&mut self, msg: ActorMessage::ProxyRequest, _ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg)
    }
}

impl<W> StreamHandler<message::ProxyResponse, io::Error> for Server<W> where W: AsyncWrite + 'static
{
    fn handle(&mut self, item: ActorMessage::ProxyResponse, _ctx: &mut Self::Context) {
        let uuid = item.uuid;
        let response = item.response;
        if uuid.is_nil() {
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
            let socks_client = self.clients.get(&uuid).unwrap();
            match response {
                ActorMessage::ProxyTransfer::Data(data) => {
                    socks_client.do_send(ActorMessage::ConnectorResponse::Data(data))
                }
                ActorMessage::ProxyTransfer::RequestAddr(_) => {
                    panic!();
                }
                ActorMessage::ProxyTransfer::Response(r) => {
                    match r {
                        ActorMessage::ProxyResponseType::Succeeded => {
                            socks_client.do_send(ActorMessage::ConnectorResponse::Succeeded)
                        }
                        ActorMessage::ProxyResponseType::ConnectionRefused => {
                            socks_client.do_send(ActorMessage::ConnectorResponse::Failed)
                        }
                        ActorMessage::ProxyResponseType::Timeout => {
                            socks_client.do_send(ActorMessage::ConnectorResponse::Failed)
                        }
                        ActorMessage::ProxyResponseType::Abort=>{
                            socks_client.do_send(ActorMessage::ConnectorResponse::Abort)
                        }
                    }
                }
                ActorMessage::ProxyTransfer::Heartbeat(_) => {
                    panic!()
                }
            }
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        info!(self.logger,"server disconnected");
        ctx.stop()
    }
}

impl<W> Handler<Heartbeat> for Server<W> where W:AsyncWrite+'static {
    type Result = ();

    fn handle(&mut self, _: Heartbeat, _ctx: &mut Self::Context) -> Self::Result {
        self.last_hb_instant=Instant::now();
        self.hb_index+=1;
        info!(self.logger,"send heartbeat {}",self.hb_index);
        self.writer.write(ActorMessage::ProxyRequest::new(
            uuid::Uuid::nil(),
            ActorMessage::ProxyTransfer::Heartbeat(self.hb_index)
        ))
    }
}

impl<W> actix::io::WriteHandler<io::Error> for Server<W> where W: AsyncWrite + 'static {}

/// Handle stream of TcpStream's
impl<W> Handler<SocksConnectedMessage> for Server<W>
    where W: AsyncWrite + 'static
{
    /// this is response for message, which is defined by `ResponseType` trait
    /// in this case we just return unit.
    type Result = ();

    fn handle(&mut self, msg: SocksConnectedMessage, ctx: &mut Context<Self>) {
        let (r, w) = msg.connector.split();
        let uuid = uuid::Uuid::new_v4();
        let uuid_key = uuid.clone();
        let server_addr = ctx.address();
        let logger=self.logger.clone();
        let addr=actix::Arbiter::start(move|ctx|{
            let uuid_str=uuid.to_string();
            SocksClient::add_stream(FramedRead::new(r, codec::Socks5RequestCodec::new()), ctx);
            let writer = actix::io::FramedWrite::new(w, codec::Socks5ResponseCodec, ctx);
            SocksClient::new(uuid,writer,server_addr,logger.new(o!("uuid"=>uuid_str)))
        });
        self.clients.insert(uuid_key, addr);
    }
}

fn connect_callback<W>(log:Logger,listener:TcpListener,proxy_address_str:String,config:opt::Config)->impl FnOnce(W)
    where W:AsyncRead+AsyncWrite+'static
{
    move|stream:W|{
        info!(log,"Connected to proxy server";"address"=>proxy_address_str.clone());
        Server::create(move |ctx| {
            ctx.add_message_stream(listener.incoming().map_err(|_| ()).map(|st| {
                SocksConnectedMessage {
                    connector: st
                }
            }));
            let (r, w) = stream.split();
            Server::add_stream(FramedRead::new(r, ActorMessage::ProxyResponseCodec::new(config.crypto.clone())), ctx);
            let writer: FramedWrite<WriteHalf<_>, ActorMessage::ProxyRequestCodec> = FramedWrite::new(w, ActorMessage::ProxyRequestCodec::new(config.crypto.clone()), ctx);
            let s=Server {
                clients: HashMap::new(),
                writer,
                hb_index:0,
                hb:Instant::now(),
                logger: log.new(o!("address"=>proxy_address_str)),
                last_hb_instant:Instant::now(),
                crypto_config:config.crypto
            };
            s.hb(ctx);
            s
        });
    }
}

fn main() {
    let opt:opt::ClientOpt = dbg!(opt::ClientOpt::from_args());

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let log = slog::Logger::root(drain, o!("version" => "0.5"));

    let quic_config = opt::QuicConfig {
        stream_window_bidi: None,
        idle_timeout: None,
        stream_receive_window: None,
        receive_window: None,
        max_tlps: None,
        packet_threshold: None,
        time_threshold: None,
        delayed_ack_timeout: None,
        initial_rtt: None,
        max_datagram_size: None,
        initial_window: None,
        minimum_window: None,
        loss_reduction_factor: None,
        presistent_cognestion_threshold: None,
        local_cid_len: None
    };
    let crypto_config= opt::CryptoConfig {
        key: "12312".to_string(),
        salt: "2121".to_string(),
        crypto_method: "aes-256-gcm".to_string(),
        digest_method: "sha256".to_string(),
        digest_iteration: 1
    };

    actix::System::run(move || {
        // Create server listener
        let addr = net::SocketAddr::from_str(&format!("{}:{}",opt.socks_host,opt.socks_port)).unwrap();
        let listener = TcpListener::bind(&addr).unwrap();
        let f=match opt.protocol.as_str() {
            "tcp"=>{
                let connector = connector::tcp::TcpConnector{};
                let proxy_address_str=format!("{}:{}",opt.proxy_host,opt.proxy_port);
                connector.connect(&proxy_address_str.clone(), connect_callback(log,listener,proxy_address_str.clone(),opt::Config{
                    quic:quic_config,
                    crypto:crypto_config.convert().unwrap()
                }))
            }
            "quic"=>{
                let connector = connector::quic::QuicClientConnector::new_dangerous();
                let proxy_address_str=format!("{}:{}",opt.proxy_host,opt.proxy_port);
                connector.connect(&proxy_address_str.clone(), connect_callback(log,listener,proxy_address_str.clone(),opt::Config{
                    quic:quic_config,
                    crypto:crypto_config.convert().unwrap()
                }))
            }
            _=>{
                panic!("unsupported protocol")
            }
        };
        actix::spawn(f);
    });
}