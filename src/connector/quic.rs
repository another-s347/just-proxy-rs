use quinn::{Endpoint, Driver, BiStream, ServerConfig};
use super::{ProxyConnector, ProxyListener};
use futures::prelude::*;
use rustls::ProtocolVersion;
use std::{fs,io};
use std::str::FromStr;
use crate::ext::OnlyFirstStream;
use actix;

pub struct QuicClientConnector {
    pub endpoint: Endpoint,
    pub driver: Driver,
}

pub struct NoCertificateVerification {}

impl rustls::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(&self,
                          _roots: &rustls::RootCertStore,
                          _presented_certs: &[rustls::Certificate],
                          _dns_name: webpki::DNSNameRef<'_>,
                          _ocsp: &[u8]) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        Ok(rustls::ServerCertVerified::assertion())
    }
}

impl QuicClientConnector {
    pub fn new_dangerous()->QuicClientConnector {
        let mut endpoint = quinn::Endpoint::new();
        let mut client_config = quinn::ClientConfigBuilder::new();
        client_config.set_protocols(&[quinn::ALPN_QUIC_HTTP]);
        //endpoint.logger(log.clone());
        client_config.enable_keylog();
        let mut cfg = client_config.build();
        let mut tls_cfg = rustls::ClientConfig::new();
        tls_cfg.dangerous().set_certificate_verifier(std::sync::Arc::new(NoCertificateVerification {}));
        tls_cfg.versions = vec![ProtocolVersion::TLSv1_3];
        cfg.tls_config = std::sync::Arc::new(tls_cfg);
        endpoint.default_client_config(cfg);
        let (endpoint, driver, _) = endpoint.bind("0.0.0.0:12311").unwrap();
        QuicClientConnector {
            endpoint,
            driver
        }
    }
}

impl ProxyConnector<BiStream> for QuicClientConnector {
    fn connect<F>(self, addr: &str, f: F) -> Box<Future<Item=(), Error=()>> where F: FnOnce(BiStream) + 'static {
        let remote = std::net::SocketAddr::from_str(addr).unwrap();
        let connect_future = self.endpoint.connect(&remote, "www.baidu.com").unwrap()
            .map_err(|e| {
                dbg!(e);
            })
            .and_then(move |conn| {
                let conn = conn.connection;
                conn.open_bi().map_err(|e| {
                    dbg!(e);
                }).and_then(move |stream| {
                    f(stream);
                    Ok(())
                })
            });
        let f = connect_future.join(self.driver.map_err(|e| eprintln!("IO error: {}", e))).map(|_| ());
        Box::new(f)
    }
}

pub struct QuicServerConnector {
    pub server_config: ServerConfig
}

impl QuicServerConnector {
    pub fn new_dangerous() -> QuicServerConnector {
        let server_config = quinn::ServerConfig {
            ..Default::default()
        };
        let mut server_config = quinn::ServerConfigBuilder::new(server_config);
        server_config.set_protocols(&[quinn::ALPN_QUIC_HTTP]);
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
                panic!()
                //bail!("failed to read certificate: {}", e);
            }
        };
        let key = quinn::PrivateKey::from_der(&key).unwrap();
        let cert = quinn::Certificate::from_der(&cert).unwrap();
        server_config.set_certificate(quinn::CertificateChain::from_certs(vec![cert]), key).unwrap();
        QuicServerConnector {
            server_config:server_config.build()
        }
    }
}

impl ProxyListener<BiStream> for QuicServerConnector {
    fn listen<F>(self, addr: &str, f: F) where F: FnOnce(Box<dyn Stream<Item=BiStream, Error=()>>) + 'static {
        let mut quic_config = quinn::Config::default();
        quic_config.idle_timeout=100;
        let mut endpoint = quinn::EndpointBuilder::new(quic_config);
        //endpoint.logger(log.clone());
        endpoint.listen(self.server_config);
        let (_, driver, incoming) = endpoint.bind(addr).unwrap();
        let s = incoming.map(move |conn| {
            let t = conn.incoming.map_err(|e| {
                dbg!(e);
            });
            OnlyFirstStream {
                inner: t,
                first_taken: false,
            }
        });
        let s2 = s.flatten().map(|newstream| {
            let stream = match newstream {
                quinn::NewStream::Bi(stream) => stream,
                quinn::NewStream::Uni(_) => unreachable!("disabled by endpoint configuration"),
            };
            //ArcStream::new(stream)
            stream
        });
        actix::spawn(driver.map_err(|e|{
            dbg!(e);
        }));
        f(Box::new(s2));
    }
}