use crate::opt;
use std::sync::Arc;
use tokio::prelude::*;
use actix::prelude::*;
use quinn::{ConnectionDriver, Connection, IncomingStreams, ServerConfigBuilder};
use slog::Drain;
use std::net::SocketAddr;
use std::any::Any;
use crate::message as ActorMessage;
use crate::opt::CryptoConfig;
use std::{fs, io};
use tokio::codec::{FramedRead, FramedWrite};
use crate::async_backend::{AsyncProxyClient, process_heartbeat};
use crate::message::ProxyResponseCodec;

pub fn run_server(addr: &str, logger: slog::Logger, config:Option<opt::Config>) {
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
    let bind_result = endpoint.bind(addr);
    if bind_result.is_err() {
        println!("error bind");
        return;
    }
    let (driver, endpoint, mut incoming) = bind_result.unwrap();
    tokio::spawn(driver.map_err(|e|{
        dbg!(e);
    }));
    let task = incoming.for_each(move|(connnectionDriver,connection,incomingStreams)|{
        let logger = logger.clone();
        handle_new_client_async(connnectionDriver,connection,incomingStreams,logger);
        Ok(())
    });
    tokio::spawn(task);
}

pub fn handle_new_client_async(connectionDriver:ConnectionDriver,connection:Connection,mut incomingStreams:IncomingStreams, logger:slog::Logger) {
    println!("new client");
    let remote_address = connection.remote_address();
    tokio::spawn(connectionDriver.map_err(|driverError|{
        dbg!(driverError);
    }));
    let client_logger = new_client_logger(remote_address.clone());
    let hb_logger = client_logger.clone();
    let hb_task = connection.open_bi().then(|result|{
        println!("openbi");
        let (sendStream,recvStream) = result.unwrap();
        let recvStream = FramedRead::new(recvStream,ActorMessage::ProxyRequestCodec::new(CryptoConfig::default().convert().unwrap()));
        let sendStream = FramedWrite::new(sendStream,ActorMessage::ProxyResponseCodec::new(CryptoConfig::default().convert().unwrap()));
        recvStream.filter_map(move|item|{
            process_heartbeat(&hb_logger,item)
        }).forward(sendStream).map_err(|e|{
            dbg!(e);
        })
    }).map(|x|{
        ()
    });
    tokio::spawn(hb_task);
    let task = incomingStreams.for_each(move|stream|{
        println!("new stream");
        let (sendStream, recvStream) = match stream {
            quinn::NewStream::Bi(send, recv) => (send, recv),
            quinn::NewStream::Uni(_) => unreachable!("disabled by endpoint configuration"),
        };
        let client_logger = new_client_logger(remote_address.clone());
        AsyncProxyClient::create(client_logger,sendStream,recvStream);
        Ok(())
    }).map_err(|e|{
        dbg!(e);
    });
    tokio::spawn(task);
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