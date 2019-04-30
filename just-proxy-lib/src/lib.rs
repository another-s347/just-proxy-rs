#![feature(toowned_clone_into)]
#![feature(await_macro, async_await, futures_api)]
#[macro_use]
extern crate failure;
extern crate structopt;
#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;

use slog::Drain;
use crate::frontend::{FrontEndServer, FrontendConnected};

pub mod message;
pub mod socks;
//pub mod connector;
pub mod opt;
pub mod transproto;
pub mod frontend;
pub mod async_backend;

//use std::net;
//use std::str::FromStr;
//use tokio::prelude::*;
//use tokio::io::WriteHalf;
use actix::prelude::*;
use std::collections::HashMap;
use tokio::net::TcpListener;
use std::net::SocketAddr;
use crate::frontend::agent::AgentType;
use tokio::io::{AsyncWrite,AsyncRead};
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;
//use actix::io::{FramedWrite};
//use tokio::codec::FramedRead;
//use tokio::net::{TcpListener};
//use std::io;
//use crate::socks::codec;
//use crate::message as ActorMessage;
//use uuid;
//use std::collections::HashMap;
//use crate::socks::SocksClient;
//use crate::socks::SocksConnectedMessage;
//use crate::connector::ProxyConnector;
//use std::time::{Duration, Instant};
//use structopt::StructOpt;
//use slog::Drain;
//use slog::*;
//use crate::component::client::connect_callback;

//pub fn run_client(opt:opt::ClientOpt,log:Logger) {
////    let opt:opt::ClientOpt = dbg!(opt::ClientOpt::from_args());
////
////    let decorator = slog_term::TermDecorator::new().build();
////    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
////    let drain = slog_async::Async::new(drain).build().fuse();
////
////    let log = slog::Logger::root(drain, o!("version" => "0.5"));
//
//    let quic_config = opt::QuicConfig {
//        stream_window_bidi: None,
//        idle_timeout: None,
//        stream_receive_window: None,
//        receive_window: None,
//        max_tlps: None,
//        packet_threshold: None,
//        time_threshold: None,
//        delayed_ack_timeout: None,
//        initial_rtt: None,
//        max_datagram_size: None,
//        initial_window: None,
//        minimum_window: None,
//        loss_reduction_factor: None,
//        presistent_cognestion_threshold: None,
//    };
//    let crypto_config= opt::CryptoConfig {
//        key: "12312".to_string(),
//        salt: "2121".to_string(),
//        crypto_method: "aes-256-gcm".to_string(),
//        digest_method: "sha256".to_string(),
//        digest_iteration: 1
//    };
//
//    actix::System::run(move || {
//        // Create server listener
//        let addr = net::SocketAddr::from_str(&format!("{}:{}",opt.socks_host,opt.socks_port)).unwrap();
//        let listener = TcpListener::bind(&addr).unwrap();
//        let f=match opt.protocol.as_str() {
//            "tcp"=>{
//                let connector = connector::tcp::TcpConnector{};
//                let proxy_address_str=format!("{}:{}",opt.proxy_host,opt.proxy_port);
//                connector.connect(&proxy_address_str.clone(), connect_callback(log,listener,proxy_address_str.clone(),opt::Config{
//                    quic:quic_config,
//                    crypto:crypto_config.convert().unwrap()
//                }))
//            }
//            "quic"=>{
//                let connector = connector::quic::QuicClientConnector::new_dangerous();
//                let proxy_address_str=format!("{}:{}",opt.proxy_host,opt.proxy_port);
//                connector.connect(&proxy_address_str.clone(), connect_callback(log,listener,proxy_address_str.clone(),opt::Config{
//                    quic:quic_config,
//                    crypto:crypto_config.convert().unwrap()
//                }))
//            }
//            _=>{
//                panic!("unsupported protocol")
//            }
//        };
//        actix::spawn(f);
//    });
//}
//
//pub fn run_server(opt:opt::ServerOpt, log:Logger) {
////    let opt: opt::ServerOpt = dbg!(opt::ServerOpt::from_args());
////
////    let decorator = slog_term::TermDecorator::new().build();
////    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
////    let drain = slog_async::Async::new(drain).build().fuse();
////
////    let log = slog::Logger::root(drain, o!("version" => "0.5"));
//
//    let quic_config = opt::QuicConfig {
//        stream_window_bidi: None,
//        idle_timeout: None,
//        stream_receive_window: None,
//        receive_window: None,
//        max_tlps: None,
//        packet_threshold: None,
//        time_threshold: None,
//        delayed_ack_timeout: None,
//        initial_rtt: None,
//        max_datagram_size: None,
//        initial_window: None,
//        minimum_window: None,
//        loss_reduction_factor: None,
//        presistent_cognestion_threshold: None
//    };
//    let crypto_config= opt::CryptoConfig {
//        key: "12312".to_string(),
//        salt: "2121".to_string(),
//        crypto_method: "aes-256-gcm".to_string(),
//        digest_method: "sha256".to_string(),
//        digest_iteration: 1
//    };
//
//    actix::System::run(move || {
//        match opt.protocol.as_str() {
//            "tcp" => {
//                let connector = connector::tcp::TcpConnector{};
//                connector.run_server(&format!("{}:{}", opt.proxy_host, opt.proxy_port),log, opt::Config {
//                    quic: quic_config,
//                    crypto: crypto_config.convert().unwrap()
//                });
//            },
//            "quic" => {
//                let connector=connector::quic::QuicServerConnector::new_dangerous();
//                connector.run_server(&format!("{}:{}", opt.proxy_host, opt.proxy_port),log, opt::Config {
//                    quic: quic_config,
//                    crypto: crypto_config.convert().unwrap()
//                });
//            },
//            _ => panic!("unsupported protocol")
//        };
//    });
//}

#[test]
fn test() {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = slog::Logger::root(drain, o!("version" => "0.6"));
    let t1 = std::thread::spawn(move||{
        let system = actix::System::new("test-server");
        tokio::run(futures::lazy(move||{
            transproto::quic::server::run_server("127.0.0.1:12346",log.clone(),None);
            Ok(())
        }));
        system.run();
    });
    let t2 = std::thread::spawn(move||{
        sleep(Duration::from_secs(1));
        let system = actix::System::new("test-client");
        let listener = TcpListener::bind(&SocketAddr::from_str("127.0.0.1:12345").unwrap()).unwrap();
        let frontend_server = FrontEndServer::create(|ctx|{
            FrontEndServer::new()
        });
        let frontend_server_addr=frontend_server.clone();
        let task = listener.incoming().for_each(move|x|{
            let port = x.peer_addr().unwrap().port();
            let (r,s) = x.split();
            frontend_server_addr.clone().do_send(FrontendConnected {
                send_stream: s,
                recv_stream: r,
                port,
                agentType: AgentType::Socks5
            });
            Ok(())
        }).map_err(|e|{
            dbg!(e);
        });
        actix::spawn(task);
        transproto::quic::client::connect("127.0.0.1:12346",frontend_server);
        system.run();
    });
    t1.join();
}