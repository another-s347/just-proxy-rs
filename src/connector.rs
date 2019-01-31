pub mod tcp;
pub mod quic;

use actix::prelude::*;
use tokio::prelude::*;
use std::io;

#[derive(Message)]
pub struct ConnectorMessage<W> {
    pub connector:W
}

pub trait ProxyConnector<W>
where W:AsyncRead+AsyncWrite+'static
{
    fn connect<F>(self,addr:&str,f:F)->Box<dyn Future<Item=(),Error=()>> where F:FnOnce(W)+'static;
}

pub trait ProxyListener<W>
where W:AsyncRead+AsyncWrite+'static
{
    fn listen<F>(self,addr:&str,f:F) where F:FnOnce(Box<dyn Stream<Item=W,Error=()>>)+'static;
}