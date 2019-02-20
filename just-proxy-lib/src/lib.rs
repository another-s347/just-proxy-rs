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
use crate::message as ActorMessage;
use uuid;
use std::collections::HashMap;
use crate::socks::SocksClient;
use crate::socks::SocksConnectedMessage;
use crate::connector::ProxyConnector;
use std::time::{Duration, Instant};
use structopt::StructOpt;
use slog::Drain;
use slog::*;
use crate::component::client::connect_callback;

pub fn run_client(opt:opt::ClientOpt,log:Logger) {
//    let opt:opt::ClientOpt = dbg!(opt::ClientOpt::from_args());
//
//    let decorator = slog_term::TermDecorator::new().build();
//    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
//    let drain = slog_async::Async::new(drain).build().fuse();
//
//    let log = slog::Logger::root(drain, o!("version" => "0.5"));

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

pub fn run_server(opt:opt::ServerOpt, log:Logger) {
//    let opt: opt::ServerOpt = dbg!(opt::ServerOpt::from_args());
//
//    let decorator = slog_term::TermDecorator::new().build();
//    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
//    let drain = slog_async::Async::new(drain).build().fuse();
//
//    let log = slog::Logger::root(drain, o!("version" => "0.5"));

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
        match opt.protocol.as_str() {
            "tcp" => {
                let connector = connector::tcp::TcpConnector{};
                connector.run_server(&format!("{}:{}", opt.proxy_host, opt.proxy_port),log, opt::Config {
                    quic: quic_config,
                    crypto: crypto_config.convert().unwrap()
                });
            },
            "quic" => {
                let connector=connector::quic::QuicServerConnector::new_dangerous();
                connector.run_server(&format!("{}:{}", opt.proxy_host, opt.proxy_port),log, opt::Config {
                    quic: quic_config,
                    crypto: crypto_config.convert().unwrap()
                });
            },
            _ => panic!("unsupported protocol")
        };
    });
}