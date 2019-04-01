use super::message;
use std::io;
use tokio::codec::*;
use bytes::BytesMut;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use std::net::{Ipv4Addr, Ipv6Addr};
use bytes::buf::BufMut;

pub struct Socks5ResponseCodec;

pub struct Socks5RequestCodec {
    state: Socks5RequestCodecState
}

pub enum Socks5RequestCodecState {
    NEGOTIATION,
    WORKING,
    READY
}

impl Socks5RequestCodec {
    pub fn new() -> Socks5RequestCodec {
        Socks5RequestCodec {
            state: Socks5RequestCodecState::NEGOTIATION
        }
    }

    fn decode_data(&mut self,src:&mut BytesMut)->Result<Option<message::SocksRequest>,io::Error>{
        if src.len() == 0 {
            return Ok(None);
        }
        //println!("receive len:{}",src.len());
        Ok(Some(message::SocksRequest::Data(src.take().freeze())))
    }

    fn decode_methodselection(&mut self, src: &mut BytesMut) -> Result<Option<message::SocksRequest>, io::Error> {
        //println!("i:{},len:{:?}",self.i,src.len());
        if src.len() < 2 {
            return Ok(None);
        }
        let n_m: &u8 = src.get(1).unwrap();
        let len = (src.len() - 2) as u8;
        if len < *n_m {
            return Ok(None);
        }
        let first_two = src.split_to(2);
        let buf = first_two.as_ref();
        let version_ref = &buf[0];
        let n_methods = buf[1].clone();
        //println!("rest:{:?}",src.as_ref());
        let rest = src.split_to(n_methods as usize);
        //println!("selected method:{:?}",rest.as_ref());
        Ok(Some(message::SocksRequest::Negotiation(message::MethodSelectionRequest {
            version: version_ref.clone(),
            n_methods,
            methods: rest.to_vec(),
        })))
    }

    fn decode_targetrequest(&mut self, src: &mut BytesMut) -> Result<Option<message::SocksRequest>, io::Error> {
        if src.len() < 5 {
            return Ok(None);
        }
        let request_type: &u8 = src.get(3).unwrap();
        let len = match request_type {
            0x01 => 10,
            0x03 => {
                let domain_len_ref: &u8 = src.get(4).unwrap();
                let domain_len = domain_len_ref.to_owned() as usize;
                7 + domain_len
            }
            0x04 => 22,
            _ => panic!()
        };
        if src.len() < len {
            return Ok(None);
        }
        let packet = src.split_to(len).to_vec();
        let version = packet[0];
        let command = packet[1];
        let reserved = packet[2];
        let request_type = packet[3];
        let typed_command = match command {
            0x01 => message::SocksRequestCommand::CONNECT,
            0x02 => message::SocksRequestCommand::BIND,
            0x03 => message::SocksRequestCommand::UdpAssociate,
            _ => panic!()
        };
        let (typed_request_type, typed_addr, port) = match request_type {
            0x01 => {
                let ipv4_bytes = BigEndian::read_u32(&packet[4..8]);
                let ipv4_addr = Ipv4Addr::from(ipv4_bytes);
                let port = BigEndian::read_i16(&packet[8..10]);
                (message::SocksRequestType::IpV4, message::SocksAddr::IPV4(ipv4_addr), port)
            }
            0x03 => {
                let domain_len = packet[4] as usize;
                let domain_bytes = &packet[5..5 + domain_len];
                let domain_string = String::from_utf8(domain_bytes.to_vec()).unwrap();
                let port = BigEndian::read_i16(&packet[5 + domain_len..7 + domain_len]);
                (message::SocksRequestType::DOMAIN, message::SocksAddr::DOMAIN(domain_string), port)
            }
            0x04 => {
                let ipv6_bytes = BigEndian::read_u128(&packet[4..20]);
                let ipv6_addr = Ipv6Addr::from(ipv6_bytes);
                let port = BigEndian::read_i16(&packet[20..22]);
                (message::SocksRequestType::IpV6, message::SocksAddr::IPV6(ipv6_addr), port)
            }
            _ => panic!()
        };
        Ok(Some(message::SocksRequest::TargetRequest(message::TargetRequest {
            version: 5,
            command: typed_command,
            reserved: 0,
            request_type: typed_request_type,
            address: typed_addr,
            port,
        })))
    }
}

impl Decoder for Socks5RequestCodec {
    type Item = message::SocksRequest;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.state {
            Socks5RequestCodecState::NEGOTIATION => {
                let r = self.decode_methodselection(src);
                match r {
                    Ok(Some(_)) => self.state = Socks5RequestCodecState::WORKING,
                    _ => {}
                }
                r
            }
            Socks5RequestCodecState::WORKING => {
                let r=self.decode_targetrequest(src);
                match r {
                    Ok(Some(_)) => self.state = Socks5RequestCodecState::READY,
                    _ => {}
                }
                r
            }
            Socks5RequestCodecState::READY=> self.decode_data(src)
        }
    }
}

impl Socks5ResponseCodec {
    fn encode_methodselection(&mut self, item: message::MethodSelectionResponse, dst: &mut BytesMut) -> Result<(), io::Error> {
        dst.reserve(2);
        dst.put_u8(item.version);
        dst.put_u8(item.method);
        Ok(())
    }

    fn encode_targetresponse(&mut self, item: message::TargetResponse, dst: &mut BytesMut) -> Result<(), io::Error> {
        let (address_bytes, response_type) = match (item.response_type, item.address) {
            (message::SocksRequestType::DOMAIN, message::SocksAddr::DOMAIN(domain)) => {
                let mut bytesmut = BytesMut::new();
                let domain_bytes = domain.as_bytes();
                let len = domain_bytes.len();
                bytesmut.reserve(len + 1);
                bytesmut.put_u8(len as u8);
                bytesmut.put(domain_bytes);
                (bytesmut, message::SocksRequestType::DOMAIN)
            }
            (message::SocksRequestType::IpV4, message::SocksAddr::IPV4(ipv4)) => {
                let mut bytesmut = BytesMut::new();
                let bytes = ipv4.octets().to_vec();
                bytesmut.reserve(4);
                bytesmut.put(bytes);
                (bytesmut, message::SocksRequestType::IpV4)
            }
            (message::SocksRequestType::IpV6, message::SocksAddr::IPV6(ipv6)) => {
                let mut bytesmut = BytesMut::new();
                let bytes = ipv6.octets().to_vec();
                bytesmut.reserve(16);
                bytesmut.put(bytes);
                (bytesmut, message::SocksRequestType::IpV6)
            }
            _ => panic!()
        };
        let len = 6 + address_bytes.len();
        dst.reserve(len);
        dst.put_u8(5);
        dst.put_u8(item.response as u8);
        dst.put_u8(0);
        dst.put_u8(match response_type {
            message::SocksRequestType::IpV4 => 0u8,
            message::SocksRequestType::IpV6 => 4u8,
            message::SocksRequestType::DOMAIN => 3u8,
            _ => panic!()
        });
        dst.put(address_bytes);
        dst.put_i16_be(item.port);
        Ok(())
    }
}

impl Encoder for Socks5ResponseCodec {
    type Item = message::SocksResponse;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            message::SocksResponse::Negotiation(i) => self.encode_methodselection(i, dst),
            message::SocksResponse::TargetResponse(i) => self.encode_targetresponse(i, dst),
            message::SocksResponse::Data(data) => {
                dst.reserve(data.len());
                dst.put(data);
                Ok(())
            }
        }
    }
}
