use actix::prelude::*;

#[derive(Message)]
pub struct ConnectorMessage<W> {
    pub connector:W
}

pub trait Connector {
    
}