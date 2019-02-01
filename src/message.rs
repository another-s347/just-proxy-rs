use tokio::codec::{Decoder, Encoder};
use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;
use packet_toolbox_rs::socks5;
use std::io;
use actix::prelude::*;
use packet_toolbox_rs::socks5::message;
use tokio::net::TcpStream;
use uuid::Uuid;
use bytes::buf::BufMut;
use std::u32;

#[derive(Message)]
pub enum ConnectorResponse {
    Succeeded,
    Failed,
    Data(Vec<u8>),
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

#[derive(Message)]
pub struct ProxyRequest {
    pub uuid: Uuid,
    // 16
    transfer_len: usize,
    // 4
    transfer_type: ProxyTransferType,
    // 1
    pub request: ProxyTransfer,
}

impl ProxyRequest {
    pub fn new(uuid: Uuid, request: ProxyTransfer) -> ProxyRequest { // split to two
        let (len, t_type) = match &request {
            ProxyTransfer::Response(_) => (1usize, ProxyTransferType::Response),
            ProxyTransfer::RequestAddr(ref a) => (a.as_bytes().len(), ProxyTransferType::RequestAddr),
            ProxyTransfer::Data(ref a) => (a.len(), ProxyTransferType::Data),
            ProxyTransfer::Heartbeat => (0, ProxyTransferType::Heartbeat)
        };
        ProxyRequest {
            uuid,
            transfer_len: len,
            transfer_type: t_type,
            request,
        }
    }
}

impl ProxyResponse {
    pub fn new(uuid: Uuid, response: ProxyTransfer) -> ProxyResponse { // split to two
        let (len, t_type) = match &response {
            ProxyTransfer::Response(_) => (1usize, ProxyTransferType::Response),
            ProxyTransfer::RequestAddr(ref a) => (a.as_bytes().len(), ProxyTransferType::RequestAddr),
            ProxyTransfer::Data(ref a) => (a.len(), ProxyTransferType::Data),
            ProxyTransfer::Heartbeat => (0, ProxyTransferType::Heartbeat)
        };
        ProxyResponse {
            uuid,
            transfer_len: len,
            transfer_type: t_type,
            response,
        }
    }
}

pub enum ProxyTransferType {
    Data = 0x0,
    RequestAddr = 0x1,
    Response = 0x2,
    Heartbeat = 0x3
}

pub enum ProxyTransfer {
    Data(Vec<u8>),
    RequestAddr(String),
    Response(ProxyResponseType),
    Heartbeat
}

pub enum ProxyResponseType {
    Succeeded = 0x0,
    ConnectionRefused = 0x1,
    Timeout = 0x2,
}

#[derive(Message)]
pub struct ProxyResponse {
    pub uuid: Uuid,
    transfer_len: usize,
    transfer_type: ProxyTransferType,
    pub response: ProxyTransfer,
}

pub struct ProxyResponseCodec;

impl tokio::codec::Decoder for ProxyResponseCodec {
    type Item = ProxyResponse;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 21 {
            return Ok(None);
        }
        let transfer_len_bytes: [u8;4] = [
            *src.get(16).unwrap(),
            *src.get(17).unwrap(),
            *src.get(18).unwrap(),
            *src.get(19).unwrap()
        ];
        let transfer_len=BigEndian::read_u32(&transfer_len_bytes);
        let all_len=(21+transfer_len) as usize;
        if src.len() < all_len {
            return Ok(None);
        }
        let mut bytes = src.split_to(all_len);
        let uuid_bytes= bytes.split_to(16);
        let transfer_type_byte:u8 = *bytes.get(4).unwrap();
        bytes.advance(5);
        let data=bytes.to_vec();
        //dbg!(&data);
        let (transfer_type,response) = match transfer_type_byte {
            0x0=>{
                (ProxyTransferType::Data,ProxyTransfer::Data(data))
            },
            0x1=>{
                (ProxyTransferType::RequestAddr,ProxyTransfer::RequestAddr(String::from_utf8(data).unwrap()))
            },
            0x2=>{
                if data.len()!=1 {
                    panic!()
                }
                let r=match data[0] {
                    0x0=>ProxyTransfer::Response(ProxyResponseType::Succeeded),
                    0x1=>ProxyTransfer::Response(ProxyResponseType::ConnectionRefused),
                    0x2=>ProxyTransfer::Response(ProxyResponseType::Timeout),
                    _=>panic!()
                };
                (ProxyTransferType::Response,r)
            },
            0x3=>{
                (ProxyTransferType::Heartbeat,ProxyTransfer::Heartbeat)
            }
            _=>panic!()
        };
        Ok(Some(ProxyResponse{
            uuid: uuid::Uuid::from_bytes(bytesmut_to_u8_16(uuid_bytes)),
            transfer_len:transfer_len as usize,
            transfer_type,
            response
        }))
    }
}

pub struct ProxyRequestCodec;

impl tokio::codec::Decoder for ProxyRequestCodec {
    type Item = ProxyRequest;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 21 {
            return Ok(None);
        }
        let transfer_len_bytes: [u8;4] = [
            *src.get(16).unwrap(),
            *src.get(17).unwrap(),
            *src.get(18).unwrap(),
            *src.get(19).unwrap()
        ];
        let transfer_len=BigEndian::read_u32(&transfer_len_bytes);
        let all_len=(21+transfer_len) as usize;
        if src.len() < all_len {
            return Ok(None);
        }
        let mut bytes = src.split_to(all_len);
        let uuid_bytes= bytes.split_to(16);
        let transfer_type_byte:u8 = *bytes.get(4).unwrap();
        bytes.advance(5);
        let data=bytes.to_vec();
        let (transfer_type,request) = match transfer_type_byte {
            0x0=>{
                (ProxyTransferType::Data,ProxyTransfer::Data(data))
            },
            0x1=>{
                (ProxyTransferType::RequestAddr,ProxyTransfer::RequestAddr(String::from_utf8(data).unwrap()))
            },
            0x2=>{
                if data.len()!=1 {
                    panic!()
                }
                let r=match data[0] {
                    0x0=>ProxyTransfer::Response(ProxyResponseType::Succeeded),
                    0x1=>ProxyTransfer::Response(ProxyResponseType::ConnectionRefused),
                    0x2=>ProxyTransfer::Response(ProxyResponseType::Timeout),
                    _=>panic!()
                };
                (ProxyTransferType::Response,r)
            },
            0x3=>{
                (ProxyTransferType::Heartbeat,ProxyTransfer::Heartbeat)
            }
            _=>panic!()
        };
        Ok(Some(ProxyRequest{
            uuid: uuid::Uuid::from_bytes(bytesmut_to_u8_16(uuid_bytes)),
            transfer_len:transfer_len as usize,
            transfer_type,
            request
        }))
    }
}

fn bytesmut_to_u8_16(src:BytesMut)->[u8;16]{
    if src.len()!=16 {
        panic!()
    }
    let mut r: [u8;16]=[0;16];
    for i in 0..16 {
        r[i]=*src.get(i).unwrap();
    }
    r
}

impl tokio::codec::Encoder for ProxyRequestCodec {
    type Item = ProxyRequest;
    type Error = io::Error;

    fn encode(&mut self, item: ProxyRequest, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let uuid_bytes = item.uuid.as_bytes();
        let mut len_buf: [u8;4] = [0;4];
        if item.transfer_len > u32::MAX as usize {
            panic!();
        }
        BigEndian::write_u32(&mut len_buf, item.transfer_len as u32);
        let len = 16 + 4 + 1 + item.transfer_len;
        dst.reserve(len);
        dst.put(uuid_bytes.to_vec());
        dst.put(len_buf.to_vec());
        dst.put(item.transfer_type as u8);
        match item.request {
            ProxyTransfer::Data(data) => {
                dst.put(data);
            }
            ProxyTransfer::RequestAddr(addr) => {
                //dbg!(&addr);
                dst.put(addr.as_bytes());
            }
            ProxyTransfer::Response(response) => {
                dst.put_u8(response as u8);
            },
            ProxyTransfer::Heartbeat=> {}
        }
        Ok(())
    }
}

impl tokio::codec::Encoder for ProxyResponseCodec {
    type Item = ProxyResponse;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let uuid_bytes = item.uuid.as_bytes();
        let mut len_buf: [u8;4] = [0;4];
        if item.transfer_len > u32::MAX as usize {
            panic!();
        }
        BigEndian::write_u32(&mut len_buf, item.transfer_len as u32);
        let len = 16 + 4 + 1 + item.transfer_len;
        dst.reserve(len);
        dst.put(uuid_bytes.to_vec());
        dst.put(len_buf.to_vec());
        dst.put(item.transfer_type as u8);
        match item.response {
            ProxyTransfer::Data(data) => {
                dst.put(data);
            }
            ProxyTransfer::RequestAddr(addr) => {
                dst.put(addr.as_bytes());
            }
            ProxyTransfer::Response(response) => {
                dst.put_u8(response as u8);
            },
            ProxyTransfer::Heartbeat => {}
        }
        Ok(())
    }
}