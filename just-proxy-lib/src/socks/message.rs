use std::net::*;
use bytes::Bytes;

#[derive(Debug)]
pub enum SocksRequest {
    Negotiation(MethodSelectionRequest),
    TargetRequest(TargetRequest),
    Data(Bytes)
}

pub enum NegotiationMethod {
    NoAuthRequir1ed,
    GSSAPI,
    UserPwd,
    IanaAssigned,
    RESERVED,
    NoAcceptable
}

#[derive(Debug)]
pub struct MethodSelectionRequest {
    pub version:u8,
    pub n_methods:u8,
    pub methods:Vec<u8>
}

pub struct MethodSelectionResponse {
    pub version:u8,
    pub method:u8
}

#[derive(Debug,Clone)]
pub struct TargetRequest {
    pub version:u8,
    pub command:SocksRequestCommand,
    pub reserved:u8,
    pub request_type:SocksRequestType,
    pub address:SocksAddr,
    pub port:i16
}

#[derive(Debug,Clone)]
pub enum SocksAddr {
    IPV4(Ipv4Addr),
    IPV6(Ipv6Addr),
    DOMAIN(String)
}

#[derive(Debug,Clone)]
pub enum SocksRequestCommand {
    CONNECT,
    BIND,
    UdpAssociate
}

#[derive(Debug,Clone)]
pub enum SocksRequestType {
    IpV4,
    DOMAIN,
    IpV6
}

pub struct TargetResponse {
    pub version:u8,
    pub response:TargetResponseType,
    pub reserved:u8,
    pub response_type:SocksRequestType,
    pub address:SocksAddr,
    pub port:i16
}

pub enum TargetResponseType {
    Succeeded=0,
    GeneralFailure=1,
    NotAllowedByRuleset=2,
    NetworkUnreachable=3,
    HostUnreachable=4,
    ConnectionRefused=5,
    TTLExpired=6,
    CommandNotSupported=7,
    AddressTypeNotSupported=8,
    Unassigned=9
}

pub enum SocksResponse {
    Negotiation(MethodSelectionResponse),
    TargetResponse(TargetResponse),
    Data(Bytes)
}

impl TargetRequest {
    pub fn to_addressstring(&self)->String {
        let s:String=match &self.address {
            SocksAddr::DOMAIN(s)=>s.to_owned(),
            SocksAddr::IPV6(i)=>{
                i.to_string()
            }
            SocksAddr::IPV4(i)=>{
                i.to_string()
            }
        };
        let port=self.port.to_string();
        format!("{}:{}",s,port)
    }
}