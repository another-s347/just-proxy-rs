use ring;
use serde::{Deserialize, Serialize};

pub struct ClientOpt {

    pub socks_host:String,

    pub proxy_host: String,

    pub socks_port: i64,

    pub proxy_port: i64,

    pub protocol:String,

    pub config:Config
}

pub struct ServerOpt {

    pub proxy_host: String,

    pub proxy_port: i64,

    pub protocol:String,

    pub config:Config
}

#[derive(Serialize, Deserialize)]
pub struct CryptoConfig {
    pub key:String,
    pub salt:String,
    pub crypto_method:String,
    pub digest_method:String,
    pub digest_iteration:u32
}

impl CryptoConfig {
    pub fn convert(self) -> Result<_CryptoConfig,&'static str> {
        let crypto_method = match self.crypto_method.as_ref() {
            "aes-256-gcm"=>&ring::aead::AES_256_GCM,
            "aes-128-gcm"=>&ring::aead::AES_128_GCM,
            "chacha20_poly1305"=>&ring::aead::CHACHA20_POLY1305,
            _=>{
                return Err("unsupported crypto method");
            }
        };
        let digest_method = match self.digest_method.as_ref() {
            "sha1"=>&ring::digest::SHA1,
            "sha256"=>&ring::digest::SHA256,
            "sha384"=>&ring::digest::SHA384,
            "sha512"=>&ring::digest::SHA512,
            "sha512_256"=>&ring::digest::SHA512_256,
            _=>{
                return Err("unsupported digest method");
            }
        };
        let key=self.key.into_bytes();
        let salt=self.salt.into_bytes();
        let digest_iteration = core::num::NonZeroU32::new(self.digest_iteration).unwrap();
        Ok(_CryptoConfig {
            key,
            salt,
            crypto_method,
            digest_method,
            digest_iteration
        })
    }
}

#[derive(Clone)]
pub struct _CryptoConfig {
    pub key:Vec<u8>,
    pub salt:Vec<u8>,
    pub crypto_method:&'static ring::aead::Algorithm,
    pub digest_method:&'static ring::digest::Algorithm,
    pub digest_iteration:core::num::NonZeroU32
}

pub struct Config {
    pub quic:QuicConfig,
    pub crypto:_CryptoConfig
}

#[derive(Serialize, Deserialize)]
pub struct QuicConfig {
    pub stream_window_bidi:Option<u64>,
    pub idle_timeout:Option<u64>,
    pub stream_receive_window:Option<u64>,
    pub receive_window:Option<u64>,
    pub max_tlps:Option<u32>,
    pub packet_threshold:Option<u32>,
    pub time_threshold:Option<u16>,
    pub delayed_ack_timeout:Option<u64>,
    pub initial_rtt:Option<u64>,
    pub max_datagram_size:Option<u64>,
    pub initial_window:Option<u64>,
    pub minimum_window:Option<u64>,
    pub loss_reduction_factor:Option<u16>,
    pub presistent_cognestion_threshold:Option<u32>,
    pub local_cid_len:Option<usize>
}

impl QuicConfig {
    pub fn to_quinn_config(&self)->quinn::Config {
        let mut config = quinn::Config::default();
        if let Some(x)=self.stream_window_bidi {
            config.stream_window_bidi=x;
        }
        if let Some(x)=self.idle_timeout {
            config.idle_timeout=x;
        }
        if let Some(x)=self.stream_receive_window{
            config.stream_receive_window=x;
        }
        if let Some(x)=self.receive_window {
            config.receive_window=x;
        }
        if let Some(x)=self.max_tlps {
            config.max_tlps=x;
        }
        if let Some(x)=self.packet_threshold {
            config.packet_threshold=x;
        }
        if let Some(x)=self.time_threshold {
            config.time_threshold=x;
        }
        if let Some(x)=self.delayed_ack_timeout {
            config.delayed_ack_timeout=x;
        }
        if let Some(x)=self.initial_rtt {
            config.initial_rtt=x;
        }
        if let Some(x)=self.max_datagram_size {
            config.max_datagram_size=x;
        }
        if let Some(x)=self.initial_window {
            config.initial_window=x;
        }
        if let Some(x)=self.minimum_window {
            config.minimum_window=x;
        }
        if let Some(x)=self.loss_reduction_factor {
            config.loss_reduction_factor=x;
        }
        if let Some(x)=self.presistent_cognestion_threshold {
            config.persistent_congestion_threshold=x;
        }
        if let Some(x)=self.local_cid_len {
            config.local_cid_len=x;
        }
        config
    }
}