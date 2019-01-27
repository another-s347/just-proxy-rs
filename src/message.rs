use tokio::codec::{Decoder, Encoder};
use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;
use packet_toolbox_rs::socks5;
use std::io;
use actix::prelude::*;
use packet_toolbox_rs::socks5::message;
use tokio::net::TcpStream;

#[derive(Message)]
pub enum ConnectorResponse {
    Successed {
        addr: message::SocksAddr,
        port: i16,
        stream: ConnectionWriter,
    },
    Failed {
        addr: message::SocksAddr,
        port: i16
    },
}

pub enum ConnectionWriter {
    Tcp(TcpStream)
}

pub struct BytesCodec;

impl tokio::codec::Decoder for BytesCodec {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() == 0 {
            return Ok(None);
        }
        Ok(Some(src.take().to_vec()))
    }
}

pub struct ProxyMessage {

}