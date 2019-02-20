use structopt::StructOpt;
use just_proxy_lib::opt;
use std::fs;
use serde_json;
use serde_json::Value;

#[derive(StructOpt, Debug)]
#[structopt(name = "bin")]
pub struct BinOpt {

    #[structopt(short = "v", long = "verbose")]
    pub verbose: bool,

    #[structopt(short = "l", long = "socks-host", default_value = "127.0.0.1")]
    pub socks_host:String,

    #[structopt(short = "S", long = "proxy-host", default_value = "127.0.0.1")]
    pub proxy_host: String,

    #[structopt(short = "p", long="socks-port", default_value = "12345")]
    pub socks_port: i64,

    #[structopt(short = "P", long = "proxy-port", default_value="12346")]
    pub proxy_port: i64,

    #[structopt(long="protocol", default_value="tcp")]
    pub protocol:String,

    #[structopt(short = "s", long="server")]
    pub server:bool,

    #[structopt(short = "c", long="config", default_value="/etc/justproxy/config.json")]
    pub config_path:String,

    #[structopt(long="dump-config")]
    pub dump_config:bool,

    #[structopt(short="r", long="retry")]
    pub retry:bool,

    #[structopt(long="retry-interval", default_value="5")]
    pub retry_interval:u64,
}

impl BinOpt {
    pub fn to_server_config(&self)->opt::ServerOpt {
        let config_string = fs::read_to_string(self.config_path.clone()).unwrap();
        let quic_config:opt::QuicConfig = serde_json::from_str(&config_string).unwrap();
        let crypto_config:opt::CryptoConfig = serde_json::from_str(&config_string).unwrap();
        opt::ServerOpt {
            proxy_host: self.proxy_host.clone(),
            proxy_port: self.proxy_port,
            protocol: self.protocol.clone(),
            config:opt::Config{
                quic: quic_config,
                crypto: crypto_config.convert().unwrap()
            }
        }
    }

    pub fn to_client_config(&self)->opt::ClientOpt {
        let config_string = fs::read_to_string(self.config_path.clone()).unwrap();
        let quic_config:opt::QuicConfig = serde_json::from_str(&config_string).unwrap();
        let crypto_config:opt::CryptoConfig = serde_json::from_str(&config_string).unwrap();
        opt::ClientOpt {
            socks_host: self.socks_host.clone(),
            proxy_host: self.proxy_host.clone(),
            socks_port: self.socks_port,
            proxy_port: self.proxy_port,
            protocol: self.protocol.clone(),
            config: opt::Config {
                quic:quic_config,
                crypto: crypto_config.convert().unwrap()
            }
        }
    }

    pub fn dump_config(){
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
        let mut quic_value=serde_json::to_value(quic_config).unwrap();
        let mut crypto_value = serde_json::to_value(crypto_config).unwrap();
        merge(&mut quic_value,&mut crypto_value);
        let string = serde_json::to_string_pretty(&quic_value).unwrap();
        print!("{}",string);
    }
}

fn merge(a: &mut Value, b: &Value) {
    match (a, b) {
        (&mut Value::Object(ref mut a), &Value::Object(ref b)) => {
            for (k, v) in b {
                merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) => {
            *a = b.clone();
        }
    }
}