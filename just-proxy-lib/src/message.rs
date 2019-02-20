#![feature(nonzero)]

use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;
use std::io;
use actix::prelude::*;
use tokio::net::TcpStream;
use uuid::Uuid;
use bytes::buf::BufMut;
use std::u32;
use bytes::Bytes;
use ring;
use core::num::NonZeroU32;
use crate::opt;

#[derive(Message)]
pub enum ConnectorResponse {
    Succeeded,
    Failed,
    Data(Bytes),
    Abort
}

pub enum ConnectionWriter {
    Tcp(TcpStream)
}

pub struct BytesCodec;

impl tokio::codec::Decoder for BytesCodec {
    type Item = Bytes;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() == 0 {
            return Ok(None);
        }
        Ok(Some(src.take().freeze()))
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
            ProxyTransfer::Heartbeat(index) => (4, ProxyTransferType::Heartbeat)
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
            ProxyTransfer::Heartbeat(index) => (4, ProxyTransferType::Heartbeat)
        };
        ProxyResponse {
            uuid,
            transfer_len: len,
            transfer_type: t_type,
            response,
        }
    }
}

#[derive(Debug)]
pub enum ProxyTransferType {
    Data = 0x0,
    RequestAddr = 0x1,
    Response = 0x2,
    Heartbeat = 0x3
}

#[derive(Debug)]
pub enum ProxyTransfer {
    Data(Bytes),
    RequestAddr(String),
    Response(ProxyResponseType),
    Heartbeat(u32)
}

#[derive(Debug)]
pub enum ProxyResponseType {
    Succeeded = 0x0,
    ConnectionRefused = 0x1,
    Timeout = 0x2,
    Abort = 0x3
}

#[derive(Message,Debug)]
pub struct ProxyResponse {
    pub uuid: Uuid,
    transfer_len: usize,
    transfer_type: ProxyTransferType,
    pub response: ProxyTransfer,
}

pub struct ProxyResponseCodec{
    crypto_algorithm:&'static ring::aead::Algorithm,
    opening_key:ring::aead::OpeningKey,
    sealing_key:ring::aead::SealingKey,
    nonce_bytes:[u8;12],
    tag_len:usize
}

impl ProxyResponseCodec {
    pub fn new(config:opt::_CryptoConfig) -> ProxyResponseCodec {
        let mut key_bytes=[0;32];
        ring::pbkdf2::derive(config.digest_method, config.digest_iteration , &config.salt, &config.key, &mut key_bytes);
        ProxyResponseCodec {
            crypto_algorithm: config.crypto_method,
            opening_key: ring::aead::OpeningKey::new(&config.crypto_method,&key_bytes).unwrap(),
            sealing_key: ring::aead::SealingKey::new(&config.crypto_method,&key_bytes).unwrap(),
            nonce_bytes: [1;12],
            tag_len: config.crypto_method.tag_len()
        }
    }
}

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
        if src.len() < all_len+self.tag_len {
            return Ok(None);
        }
        let mut bytes = src.split_to(all_len+self.tag_len);
        let uuid_bytes= bytes.split_to(16);
        bytes.advance(4);
        let mut key_bytes = [0; 32];
        let nonce=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let aad = ring::aead::Aad::from(uuid_bytes.as_ref());
        let mut dec_place=bytes.as_mut();
        let dec_bytes_result=ring::aead::open_in_place(&self.opening_key,nonce,aad,0,dec_place);
        let dec_bytes=if let Ok(dec)=dec_bytes_result {
            dec
        }
        else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"dec error"));
        };
        let transfer_type_byte = dec_bytes[0];
        bytes.advance(1);
        let data= bytes.split_to(transfer_len as usize).freeze();
        let (transfer_type,response) = match transfer_type_byte {
            0x0=>{
                (ProxyTransferType::Data,ProxyTransfer::Data(data))
            },
            0x1=>{
                (ProxyTransferType::RequestAddr,ProxyTransfer::RequestAddr(String::from_utf8(data.to_vec()).unwrap()))
            },
            0x2=>{
                if data.len()!=1 {
                    panic!()
                }
                let r=match data[0] {
                    0x0=>ProxyTransfer::Response(ProxyResponseType::Succeeded),
                    0x1=>ProxyTransfer::Response(ProxyResponseType::ConnectionRefused),
                    0x2=>ProxyTransfer::Response(ProxyResponseType::Timeout),
                    0x3=>ProxyTransfer::Response(ProxyResponseType::Abort),
                    _=>panic!()
                };
                (ProxyTransferType::Response,r)
            },
            0x3=>{
                let index:u32=byteorder::BigEndian::read_u32(data.as_ref());
                (ProxyTransferType::Heartbeat,ProxyTransfer::Heartbeat(index))
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

pub struct ProxyRequestCodec{
    crypto_algorithm:&'static ring::aead::Algorithm,
    opening_key:ring::aead::OpeningKey,
    sealing_key:ring::aead::SealingKey,
    nonce_bytes:[u8;12],
    tag_len:usize
}

impl ProxyRequestCodec {
    pub fn new(config:opt::_CryptoConfig) -> ProxyRequestCodec {
        let mut key_bytes=[0;32];
        ring::pbkdf2::derive(config.digest_method, config.digest_iteration , &config.salt, &config.key, &mut key_bytes);
        ProxyRequestCodec {
            crypto_algorithm: config.crypto_method,
            opening_key: ring::aead::OpeningKey::new(&config.crypto_method,&key_bytes).unwrap(),
            sealing_key: ring::aead::SealingKey::new(&config.crypto_method,&key_bytes).unwrap(),
            nonce_bytes: [1;12],
            tag_len: config.crypto_method.tag_len()
        }
    }
}
//pub struct ProxyRequestCodec;

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
        //println!("len:{}",transfer_len);
        let all_len=(21+transfer_len) as usize;
        if src.len() < all_len+self.tag_len {
            return Ok(None);
        }
        let mut bytes = src.split_to(all_len+self.tag_len);
        let uuid_bytes= bytes.split_to(16);
        bytes.advance(4);
        let mut key_bytes = [0; 32];
        let nonce=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let aad = ring::aead::Aad::from(uuid_bytes.as_ref());
        let mut dec_place=bytes.as_mut();
        let dec_bytes_result=ring::aead::open_in_place(&self.opening_key,nonce,aad,0,dec_place);
        let dec_bytes=if let Ok(dec)=dec_bytes_result {
            dec
        }
        else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"dec error"));
        };
        let transfer_type_byte = dec_bytes[0];
        bytes.advance(1);
        let data= bytes.split_to(transfer_len as usize).freeze();
        let (transfer_type,request) = match transfer_type_byte {
            0x0=>{
                (ProxyTransferType::Data,ProxyTransfer::Data(data))
            },
            0x1=>{
                (ProxyTransferType::RequestAddr,ProxyTransfer::RequestAddr(String::from_utf8(data.to_vec()).unwrap()))
            },
            0x2=>{
                if data.len()!=1 {
                    panic!()
                }
                let r=match data[0] {
                    0x0=>ProxyTransfer::Response(ProxyResponseType::Succeeded),
                    0x1=>ProxyTransfer::Response(ProxyResponseType::ConnectionRefused),
                    0x2=>ProxyTransfer::Response(ProxyResponseType::Timeout),
                    0x3=>ProxyTransfer::Response(ProxyResponseType::Abort),
                    _=>panic!()
                };
                (ProxyTransferType::Response,r)
            },
            0x3=>{
                let index:u32=byteorder::BigEndian::read_u32(data.as_ref());
                (ProxyTransferType::Heartbeat,ProxyTransfer::Heartbeat(index))
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
        //let tag_len=ring::aead::AES_256_GCM.tag_len();
        BigEndian::write_u32(&mut len_buf, item.transfer_len as u32);
        //println!("len:{}",item.transfer_len);
        let len = 16 + 4 + 1 + item.transfer_len;
        let mut raw_dst=BytesMut::new();
        raw_dst.reserve(len+self.tag_len-16-4);
        dst.reserve(len+self.tag_len);
        //dbg!(&uuid_bytes);
        dst.put(uuid_bytes.to_vec());
        dst.put(len_buf.to_vec());
        raw_dst.put(item.transfer_type as u8);
        match item.request {
            ProxyTransfer::Data(data) => {
                raw_dst.put(data);
            }
            ProxyTransfer::RequestAddr(addr) => {
                //dbg!(&addr);
                raw_dst.put(addr.as_bytes());
            }
            ProxyTransfer::Response(response) => {
                raw_dst.put_u8(response as u8);
            },
            ProxyTransfer::Heartbeat(index)=> {
                raw_dst.put_u32_be(index);
            }
        }
        //dbg!(&raw_dst);
        for _ in 0..self.tag_len {
            raw_dst.put(0u8);
        }
        let enc_place=raw_dst.as_mut();
        let mut key_bytes = [0; 32];
        let nonce=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let aad = ring::aead::Aad::from(uuid_bytes);
        let result=ring::aead::seal_in_place(&self.sealing_key,nonce,aad,enc_place,self.tag_len);
        if result.is_err() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"enc error"));
        }
//        dbg!(&enc_place);
        dst.put(raw_dst);
//        dbg!(&dst);
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
        let mut raw_dst=BytesMut::new();
        raw_dst.reserve(len+self.tag_len-16-4);
        dst.reserve(len+self.tag_len);
        dst.put(uuid_bytes.to_vec());
        dst.put(len_buf.to_vec());
        raw_dst.put(item.transfer_type as u8);
        match item.response {
            ProxyTransfer::Data(data) => {
                raw_dst.put(data);
            }
            ProxyTransfer::RequestAddr(addr) => {
                raw_dst.put(addr.as_bytes());
            }
            ProxyTransfer::Response(response) => {
                raw_dst.put_u8(response as u8);
            },
            ProxyTransfer::Heartbeat(index) => {
                raw_dst.put_u32_be(index);
            }
        }
        for _ in 0..self.tag_len {
            raw_dst.put(0u8);
        }
        let enc_place=raw_dst.as_mut();
        let mut key_bytes = [0; 32];
        let nonce=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let aad = ring::aead::Aad::from(uuid_bytes);
        let result=ring::aead::seal_in_place(&self.sealing_key,nonce,aad,enc_place,self.tag_len);
        if result.is_err() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"enc error"));
        }
//        dbg!(&enc_place);
        dst.put(raw_dst);
//        dbg!(&dst);
        Ok(())
    }
}