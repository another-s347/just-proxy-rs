use tokio::prelude::*;
use std::str::FromStr;
use slog::Drain;
use actix::prelude::*;
use crate::frontend::{FrontEndServer, ProtocolServerConnected, HeartbeatActor};
use actix::Addr;
use crate::transproto::quic::NoCertificateVerification;
use std::sync::Arc;
use rustls::ProtocolVersion;
use tokio::codec::{FramedWrite, FramedRead};
use crate::message::{ProxyResponseCodec, ProxyRequestCodec};
use crate::opt::CryptoConfig;
use crate::message::ProxyTransferType::Heartbeat;
use std::time::Instant;

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
    let (endpoint_driver, endpoint, incoming) = endpoint.bind("[::]:0").unwrap();
    let t=incoming.for_each(|(driver,conn,incoming)|{
        actix::spawn(driver.map_err(|e|{
            dbg!(e);
        }));
        actix::spawn(incoming.for_each(|x|{
            println!("new stream client");
            Ok(())
        }).map_err(|e|{
            dbg!(e);
        }));
        Ok(())
    });
    actix::spawn(t);
    actix::spawn(endpoint_driver.map_err(|e| eprintln!("IO error: {}", e)));
    let connect_future = endpoint.connect(&remote, "www.baidu.com").unwrap()
        .map_err(|e| {
            dbg!(e);
        })
        .and_then(move |(driver,connection,incomingstream)| {
            actix::spawn(driver.map_err(|e|{
                dbg!(e);
            }));
            println!("server connected");
            frontend_server.do_send(ProtocolServerConnected(connection));
            let hb_task = incomingstream.for_each(|x|{
                println!("newstream");
                let hbstream = x;
                let (s,r) = match hbstream {
                    quinn::NewStream::Bi(send, recv) => (send, recv),
                    quinn::NewStream::Uni(_) => unreachable!("disabled by endpoint configuration"),
                };
                let r = FramedRead::new(r,ProxyResponseCodec::new(CryptoConfig::default().convert().unwrap()));
                let s = FramedWrite::new(s,ProxyRequestCodec::new(CryptoConfig::default().convert().unwrap()));
                HeartbeatActor::create(|ctx|{
                    ctx.add_stream(r);
                    HeartbeatActor::new(s)
                });
                Ok(())
            }).map_err(|e|{
                dbg!(e);
            });
            actix::spawn(hb_task);
            Ok(())
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