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
use ring::rand::SecureRandom;

#[derive(Message)]
pub enum ConnectorResponse {
    Succeeded,
    Failed,
    Data(Bytes),
    Abort,
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
    pub uuid: u16,
    // 16
    transfer_len: usize,
    // 4
    transfer_type: ProxyTransferType,
    // 1
    pub request: ProxyTransfer,
}

impl ProxyRequest {
    pub fn new(uuid: u16, request: ProxyTransfer) -> ProxyRequest { // split to two
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
    pub fn new(uuid: u16, response: ProxyTransfer) -> ProxyResponse { // split to two
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
    Heartbeat = 0x3,
}

#[derive(Debug)]
pub enum ProxyTransfer {
    Data(Bytes),
    RequestAddr(String),
    Response(ProxyResponseType),
    Heartbeat(u32),
}

#[derive(Debug)]
pub enum ProxyResponseType {
    Succeeded = 0x0,
    ConnectionRefused = 0x1,
    Timeout = 0x2,
    Abort = 0x3,
}

#[derive(Message, Debug)]
pub struct ProxyResponse {
    pub uuid: u16,
    transfer_len: usize,
    transfer_type: ProxyTransferType,
    pub response: ProxyTransfer,
}

pub struct ProxyResponseCodec {
    crypto_algorithm: &'static ring::aead::Algorithm,
    opening_key: ring::aead::OpeningKey,
    sealing_key: ring::aead::SealingKey,
    nonce_bytes: [u8; 12],
    tag_len: usize,
}

impl ProxyResponseCodec {
    pub fn new(config: opt::_CryptoConfig) -> ProxyResponseCodec {
        let mut key_bytes = [0; 32];
        ring::pbkdf2::derive(config.digest_method, config.digest_iteration, &config.salt, &config.key, &mut key_bytes);
        ProxyResponseCodec {
            crypto_algorithm: config.crypto_method,
            opening_key: ring::aead::OpeningKey::new(&config.crypto_method, &key_bytes).unwrap(),
            sealing_key: ring::aead::SealingKey::new(&config.crypto_method, &key_bytes).unwrap(),
            nonce_bytes: [1; 12],
            tag_len: config.crypto_method.tag_len(),
        }
    }
}

impl tokio::codec::Decoder for ProxyResponseCodec {
    type Item = ProxyResponse;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // parse 1 decode
        if src.len() < 6 + 4 + 1 + self.tag_len {
            return Ok(None);
        }
        let nonce_1 = ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let zero_aad = ring::aead::Aad::from(&[]);
        let mut parse1_dec_place=vec![0u8;11+self.tag_len];
        src.chunks(11+self.tag_len).next().unwrap().clone_into(&mut parse1_dec_place);
        let parse1_dec_bytes_result = ring::aead::open_in_place(&self.opening_key, nonce_1, zero_aad, 0, &mut parse1_dec_place);
        let parse1_dec_bytes = if let Ok(dec) = parse1_dec_bytes_result {
            dec
        } else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "dec error"));
        };
        let transfer_len_bytes: [u8;4] = [
            *parse1_dec_bytes.get(6).unwrap(),
            *parse1_dec_bytes.get(7).unwrap(),
            *parse1_dec_bytes.get(8).unwrap(),
            *parse1_dec_bytes.get(9).unwrap()
        ];
        let transfer_len=BigEndian::read_u32(&transfer_len_bytes);
        let transfer_type_byte:u8 = parse1_dec_bytes.get(10).unwrap().clone();
        //let uuid_bytes = u8_to_u8_2(&parse1_dec_place);
        let uuid = BigEndian::read_u16(&parse1_dec_bytes);

        // parse 2 decode
        if src.len() < 6 + 4 + 1 + self.tag_len + transfer_len as usize + self.tag_len {
            return Ok(None);
        }
        let mut packet=src.split_to(6 + 4 + 1 + self.tag_len + transfer_len as usize + self.tag_len);
        let parse1_bytes = packet.split_to(11+self.tag_len);
        let nonce_2=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let aad = ring::aead::Aad::from(parse1_bytes.as_ref());
        let parse2_bytes=packet.as_mut();
        let dec_bytes_result=ring::aead::open_in_place(&self.opening_key,nonce_2,aad,0,parse2_bytes);
        let parse2_dec_bytes=if let Ok(dec)=dec_bytes_result {
            dec
        }
        else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"dec error"));
        };
        let (transfer_type,request) = match transfer_type_byte {
            0x0=>{
                packet.truncate(packet.len()-self.tag_len);
                (ProxyTransferType::Data,ProxyTransfer::Data(packet.freeze()))
            },
            0x1=>{
                (ProxyTransferType::RequestAddr,ProxyTransfer::RequestAddr(String::from_utf8(Vec::from(parse2_dec_bytes)).unwrap()))
            },
            0x2=>{
                if parse2_dec_bytes.len()!=1 {
                    panic!()
                }
                let r=match parse2_dec_bytes[0] {
                    0x0=>ProxyTransfer::Response(ProxyResponseType::Succeeded),
                    0x1=>ProxyTransfer::Response(ProxyResponseType::ConnectionRefused),
                    0x2=>ProxyTransfer::Response(ProxyResponseType::Timeout),
                    0x3=>ProxyTransfer::Response(ProxyResponseType::Abort),
                    _=>panic!()
                };
                (ProxyTransferType::Response,r)
            },
            0x3=>{
                let index:u32=byteorder::BigEndian::read_u32(parse2_dec_bytes);
                (ProxyTransferType::Heartbeat,ProxyTransfer::Heartbeat(index))
            }
            _=>panic!()
        };
        Ok(Some(ProxyResponse{
            uuid,
            transfer_len:transfer_len as usize,
            transfer_type,
            response:request
        }))
    }
}

pub struct ProxyRequestCodec {
    crypto_algorithm: &'static ring::aead::Algorithm,
    opening_key: ring::aead::OpeningKey,
    sealing_key: ring::aead::SealingKey,
    nonce_bytes: [u8; 12],
    tag_len: usize,
}

impl ProxyRequestCodec {
    pub fn new(config: opt::_CryptoConfig) -> ProxyRequestCodec {
        let mut key_bytes = [0; 32];
        ring::pbkdf2::derive(config.digest_method, config.digest_iteration, &config.salt, &config.key, &mut key_bytes);
        ProxyRequestCodec {
            crypto_algorithm: config.crypto_method,
            opening_key: ring::aead::OpeningKey::new(&config.crypto_method, &key_bytes).unwrap(),
            sealing_key: ring::aead::SealingKey::new(&config.crypto_method, &key_bytes).unwrap(),
            nonce_bytes: [1; 12],
            tag_len: config.crypto_method.tag_len(),
        }
    }
}

impl tokio::codec::Decoder for ProxyRequestCodec {
    type Item = ProxyRequest;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // parse 1 decode
        if src.len() < 6 + 4 + 1 + self.tag_len {
            return Ok(None);
        }
        let nonce_1 = ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let zero_aad = ring::aead::Aad::from(&[]);
        let mut parse1_dec_place=vec![0u8;11+self.tag_len];
        src.chunks(11+self.tag_len).next().unwrap().clone_into(&mut parse1_dec_place);
        let parse1_dec_bytes_result = ring::aead::open_in_place(&self.opening_key, nonce_1, zero_aad, 0, &mut parse1_dec_place);
        let parse1_dec_bytes = if let Ok(dec) = parse1_dec_bytes_result {
            dec
        } else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "dec error"));
        };
        let transfer_len_bytes: [u8;4] = [
            *parse1_dec_bytes.get(6).unwrap(),
            *parse1_dec_bytes.get(7).unwrap(),
            *parse1_dec_bytes.get(8).unwrap(),
            *parse1_dec_bytes.get(9).unwrap()
        ];
        let transfer_len=BigEndian::read_u32(&transfer_len_bytes);
        let transfer_type_byte:u8 = parse1_dec_bytes.get(10).unwrap().clone();
        //let uuid_bytes = u8_to_u8_2(&parse1_dec_place);
        let uuid = BigEndian::read_u16(&parse1_dec_bytes);

        // parse 2 decode
        if src.len() < 6 + 4 + 1 + self.tag_len + transfer_len as usize + self.tag_len {
            return Ok(None);
        }
        let mut packet=src.split_to(6 + 4 + 1 + self.tag_len + transfer_len as usize + self.tag_len);
        let parse1_bytes = packet.split_to(11+self.tag_len);
        let nonce_2=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let aad = ring::aead::Aad::from(parse1_bytes.as_ref());
        let parse2_bytes=packet.as_mut();
        let dec_bytes_result=ring::aead::open_in_place(&self.opening_key,nonce_2,aad,0,parse2_bytes);
        let parse2_dec_bytes=if let Ok(dec)=dec_bytes_result {
            dec
        }
        else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"dec error"));
        };
        let (transfer_type,request) = match transfer_type_byte {
            0x0=>{
                packet.truncate(packet.len()-self.tag_len);
                (ProxyTransferType::Data,ProxyTransfer::Data(packet.freeze()))
            },
            0x1=>{
                (ProxyTransferType::RequestAddr,ProxyTransfer::RequestAddr(String::from_utf8(Vec::from(parse2_dec_bytes)).unwrap()))
            },
            0x2=>{
                if parse2_dec_bytes.len()!=1 {
                    panic!()
                }
                let r=match parse2_dec_bytes[0] {
                    0x0=>ProxyTransfer::Response(ProxyResponseType::Succeeded),
                    0x1=>ProxyTransfer::Response(ProxyResponseType::ConnectionRefused),
                    0x2=>ProxyTransfer::Response(ProxyResponseType::Timeout),
                    0x3=>ProxyTransfer::Response(ProxyResponseType::Abort),
                    _=>panic!()
                };
                (ProxyTransferType::Response,r)
            },
            0x3=>{
                let index:u32=byteorder::BigEndian::read_u32(parse2_dec_bytes);
                (ProxyTransferType::Heartbeat,ProxyTransfer::Heartbeat(index))
            }
            _=>panic!()
        };
        Ok(Some(ProxyRequest{
            uuid,
            transfer_len:transfer_len as usize,
            transfer_type,
            request
        }))
    }
}

fn bytesmut_to_u8_16(src: BytesMut) -> [u8; 16] {
    if src.len() != 16 {
        panic!()
    }
    let mut r: [u8; 16] = [0; 16];
    for i in 0..16 {
        r[i] = *src.get(i).unwrap();
    }
    r
}

fn u8_to_u8_2(src: &[u8]) -> [u8; 2] {
    if src.len() < 2 {
        panic!()
    }
    let mut r: [u8; 2] = [0; 2];
    for i in 0..2 {
        r[i] = src[i];
    }
    r
}

impl tokio::codec::Encoder for ProxyRequestCodec {
    type Item = ProxyRequest;
    type Error = io::Error;

    fn encode(&mut self, item: ProxyRequest, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // parse 1 encode

        // prepare parse 1 data
        let uuid_bytes = item.uuid;
        let mut random = vec![0u8;4];
        let generator = ring::rand::SystemRandom::new();
        generator.fill(&mut random).unwrap();
        let mut len_buf: [u8;4] = [0;4];
        BigEndian::write_u32(&mut len_buf, item.transfer_len as u32);
        let mut parse1_buf=BytesMut::new();
        parse1_buf.reserve(6+4+1+self.tag_len);
        parse1_buf.put_u16_be(uuid_bytes);
        parse1_buf.put(random);
        parse1_buf.put(len_buf.to_vec());
        parse1_buf.put(item.transfer_type as u8);
        for _ in 0..self.tag_len {
            parse1_buf.put(0u8);
        }
        // encrypt parse 1
        let mut parse1_place=parse1_buf.as_mut();
        let nonce1=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let zero_aad = ring::aead::Aad::from(&[]);
        let result=ring::aead::seal_in_place(&self.sealing_key,nonce1,zero_aad,&mut parse1_place,self.tag_len);
        if result.is_err() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"enc error"));
        }
        let parse2_buf={
            let aad = ring::aead::Aad::from(parse1_place);
            // parse 2 encode
            let mut parse2_buf=BytesMut::new();
            parse2_buf.reserve(item.transfer_len+self.tag_len);
            match item.request {
                ProxyTransfer::Data(data) => {
                    parse2_buf.put(data);
                }
                ProxyTransfer::RequestAddr(addr) => {
                    parse2_buf.put(addr.as_bytes());
                }
                ProxyTransfer::Response(response) => {
                    parse2_buf.put_u8(response as u8);
                },
                ProxyTransfer::Heartbeat(index)=> {
                    parse2_buf.put_u32_be(index);
                }
            }
            for _ in 0..self.tag_len {
                parse2_buf.put(0u8);
            }
            let mut parse2_place=parse2_buf.as_mut();
            let nonce_2=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
            let result=ring::aead::seal_in_place(&self.sealing_key,nonce_2,aad,parse2_place,self.tag_len);
            if result.is_err() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput,"enc error"));
            }
            parse2_buf
        };
        dst.reserve(6+4+1+self.tag_len+item.transfer_len+self.tag_len);
        dst.put(parse1_place);
        dst.put(parse2_buf);
        Ok(())
    }
}

impl tokio::codec::Encoder for ProxyResponseCodec {
    type Item = ProxyResponse;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // parse 1 encode

        // prepare parse 1 data
        let uuid_bytes = item.uuid;
        let mut random = vec![0u8;4];
        let generator = ring::rand::SystemRandom::new();
        generator.fill(&mut random).unwrap();
        let mut len_buf: [u8;4] = [0;4];
        BigEndian::write_u32(&mut len_buf, item.transfer_len as u32);
        let mut parse1_buf=BytesMut::new();
        parse1_buf.reserve(6+4+1+self.tag_len);
        parse1_buf.put_u16_be(uuid_bytes);
        parse1_buf.put(random);
        parse1_buf.put(len_buf.to_vec());
        parse1_buf.put(item.transfer_type as u8);
        for _ in 0..self.tag_len {
            parse1_buf.put(0u8);
        }
        // encrypt parse 1
        let mut parse1_place=parse1_buf.as_mut();
        let nonce1=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
        let zero_aad = ring::aead::Aad::from(&[]);
        let result=ring::aead::seal_in_place(&self.sealing_key,nonce1,zero_aad,&mut parse1_place,self.tag_len);
        if result.is_err() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"enc error"));
        }
        let parse2_buf={
            let aad = ring::aead::Aad::from(parse1_place);
            // parse 2 encode
            let mut parse2_buf=BytesMut::new();
            parse2_buf.reserve(item.transfer_len+self.tag_len);
            match item.response {
                ProxyTransfer::Data(data) => {
                    parse2_buf.put(data);
                }
                ProxyTransfer::RequestAddr(addr) => {
                    parse2_buf.put(addr.as_bytes());
                }
                ProxyTransfer::Response(response) => {
                    parse2_buf.put_u8(response as u8);
                },
                ProxyTransfer::Heartbeat(index)=> {
                    parse2_buf.put_u32_be(index);
                }
            }
            for _ in 0..self.tag_len {
                parse2_buf.put(0u8);
            }
            let mut parse2_place=parse2_buf.as_mut();
            let nonce_2=ring::aead::Nonce::try_assume_unique_for_key(&self.nonce_bytes).unwrap();
            let result=ring::aead::seal_in_place(&self.sealing_key,nonce_2,aad,parse2_place,self.tag_len);
            if result.is_err() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput,"enc error"));
            }
            parse2_buf
        };
        dst.reserve(6+4+1+self.tag_len+item.transfer_len+self.tag_len);
        dst.put(parse1_place);
        dst.put(parse2_buf);
        Ok(())
    }
}