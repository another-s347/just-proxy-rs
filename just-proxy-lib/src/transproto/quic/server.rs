use crate::opt;
use std::sync::Arc;
use tokio::prelude::*;
use actix::prelude::*;
use crate::backend::ProxyClient;
use quinn::{ConnectionDriver, Connection, IncomingStreams, ServerConfigBuilder};
use slog::Drain;
use std::net::SocketAddr;
use std::any::Any;
use crate::message as ActorMessage;
use crate::opt::CryptoConfig;
use std::{fs, io};
use tokio::codec::FramedRead;
use crate::async_backend::{AsyncProxyClient, read_stream};

pub async fn run_server(addr: &str, logger: slog::Logger, config:Option<opt::Config>) {
    let server_config = quinn::ServerConfig {
        transport: Arc::new(quinn::TransportConfig {
            stream_window_uni: 0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut server_config = quinn::ServerConfigBuilder::new(server_config);
    server_config.protocols(&[quinn::ALPN_QUIC_HTTP]);
    set_cert(&mut server_config);
    let mut endpoint = quinn::EndpointBuilder::default();
    endpoint.logger(logger.new(o!("quic"=>addr.to_owned())));
    endpoint.listen(server_config.build());
    let (driver, endpoint, mut incoming) = endpoint.bind(addr).unwrap();
//    actix::spawn(driver.map_err(|e|{
//        dbg!(e);
//    }));
    tokio::spawn(driver.map_err(|e|{
        dbg!(e);
    }));
    while let Some(Ok((connectionDriver,connection,incomingStreams))) = await!(incoming.next()) {
        let logger = logger.clone();
        tokio::spawn_async(async move {
            await!(handle_new_client_async(connectionDriver,connection,incomingStreams,logger));
        });
    }
//    let s = incoming.for_each(move |(connectionDriver,connection,incomingStreams)| {
//        handle_new_client(connectionDriver,connection,incomingStreams,logger.clone());
//        Ok(())
//    });
//    actix::spawn(s);
//    actix::spawn(s);
}

pub async fn handle_new_client_async(connectionDriver:ConnectionDriver,connection:Connection,mut incomingStreams:IncomingStreams, logger:slog::Logger) {
    let remote_address = connection.remote_address();
    tokio::spawn(connectionDriver.map_err(|driverError|{
        dbg!(driverError);
    }));
    while let Some(Ok((stream))) = await!(incomingStreams.next()) {
        println!("new stream");
        let (sendStream, recvStream) = match stream {
            quinn::NewStream::Bi(send, recv) => (send, recv),
            quinn::NewStream::Uni(_) => unreachable!("disabled by endpoint configuration"),
        };
        let client_logger = new_client_logger(remote_address.clone());
        let mut async_client = AsyncProxyClient::new(client_logger,sendStream);
        tokio::spawn_async(read_stream(async_client,recvStream));
    }
}

pub fn handle_new_client(connectionDriver:ConnectionDriver,connection:Connection,incomingStreams:IncomingStreams, logger:slog::Logger) {
    actix::Arbiter::spawn_fn(move||{
        let remote_address = connection.remote_address();
        let streamsTask = incomingStreams.for_each(move|stream|{
            println!("new stream");
            let (sendStream, recvStream) = match stream {
                quinn::NewStream::Bi(send, recv) => (send, recv),
                quinn::NewStream::Uni(_) => unreachable!("disabled by endpoint configuration"),
            };
            let client_logger = new_client_logger(remote_address.clone());
            ProxyClient::create(move|ctx|{
                // TODO: deal with error gracefully
                let (client,receiver) = ProxyClient::new(client_logger.clone());
                let sink = tokio::codec::FramedWrite::new(sendStream,ActorMessage::ProxyResponseCodec::new(CryptoConfig::default().convert().unwrap()));
                let write_task = receiver.forward(sink.sink_map_err(|sinkError|{
                    dbg!(sinkError);
                })).map(|_|());
                ctx.add_stream(FramedRead::new(recvStream, ActorMessage::ProxyRequestCodec::new(CryptoConfig::default().convert().unwrap())));
                ctx.spawn(write_task.into_actor(&client));
                client
            });
            Ok(())
        }).map_err(|incomingStreamErr|{
            dbg!(incomingStreamErr);
        });
        actix::spawn(streamsTask);
        actix::spawn(connectionDriver.map_err(|driverError|{
            dbg!(driverError);
        }));
        Ok(())
    });
}

pub fn new_client_logger(addr:SocketAddr)->slog::Logger{
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = slog::Logger::root(drain, o!("client"=>addr));
    log
}

pub fn set_cert(server_config:&mut ServerConfigBuilder) {
    let dirs = directories::ProjectDirs::from("org", "quinn", "quinn-examples").unwrap();
    let path = dirs.data_local_dir();
    let cert_path = path.join("cert.der");
    let key_path = path.join("key.der");
    let (cert, key): (Vec<u8>, Vec<u8>) = match fs::read(&cert_path).and_then(|x| Ok((x, fs::read(&key_path).unwrap()))) {
        Ok(x) => x,
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
            //info!(log, "generating self-signed certificate");
            let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]);
            let key = cert.serialize_private_key_der();
            let cert = cert.serialize_der();
            fs::create_dir_all(&path).map_err(|_| format_err!("failed to create certificate directory")).unwrap();
            fs::write(&cert_path, &cert).map_err(|_| format_err!("failed to write certificate")).unwrap();
            fs::write(&key_path, &key).map_err(|_| format_err!("failed to write private key")).unwrap();
            (cert, key)
        }
        Err(e) => {
            panic!(e)
            //bail!("failed to read certificate: {}", e);
        }
    };
    let key = quinn::PrivateKey::from_der(&key).unwrap();
    let cert = quinn::Certificate::from_der(&cert).unwrap();
    server_config.certificate(quinn::CertificateChain::from_certs(vec![cert]), key).unwrap();
}