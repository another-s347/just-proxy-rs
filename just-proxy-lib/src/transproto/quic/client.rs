use tokio::prelude::*;
use std::str::FromStr;
use slog::Drain;
use crate::frontend::{FrontEndServer, ProtocolServerConnected};
use actix::Addr;
use crate::transproto::quic::NoCertificateVerification;
use std::sync::Arc;
use rustls::ProtocolVersion;

pub fn connect(addr: &str, frontend_server:Addr<FrontEndServer>) {
    let remote = std::net::SocketAddr::from_str(addr).unwrap();
    let mut endpoint = quinn::EndpointBuilder::default();
    endpoint.logger(new_client_logger());
    let mut client_config = quinn::ClientConfig::default();
    let mut crypto_config = rustls::ClientConfig::new();
    crypto_config.versions = vec![ProtocolVersion::TLSv1_3];
    let mut dangerous = crypto_config.dangerous();
    dangerous.set_certificate_verifier(Arc::new(NoCertificateVerification{}));
    client_config.crypto=Arc::new(crypto_config);
    let mut client_config = quinn::ClientConfigBuilder::new(client_config);
    client_config.protocols(&[quinn::ALPN_QUIC_HTTP]);
    endpoint.default_client_config(client_config.build());
    let (endpoint_driver, endpoint, _) = endpoint.bind("[::]:0").unwrap();
    actix::spawn(endpoint_driver.map_err(|e| eprintln!("IO error: {}", e)));
    let connect_future = endpoint.connect(&remote, "www.baidu.com").unwrap()
        .map_err(|e| {
            dbg!(e);
        })
        .and_then(move |(driver,connection,incomingstream)| {
            let conn = connection;
            actix::spawn(driver.map_err(|e|()));
            conn.open_bi().map_err(|e| {
                dbg!(e);
            }).and_then(move |(send_stream, recv_stream)| {
                frontend_server.do_send(ProtocolServerConnected {
                    send_stream,
                    recv_stream
                });
                Ok(())
            })
        });
    actix::spawn(connect_future);
}

pub fn new_client_logger()->slog::Logger{
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = slog::Logger::root(drain, o!("c"=>1));
    log
}