use tokio::io::{AsyncWrite,AsyncRead};
use std::io;

pub struct CryptoStream<T>
where T:AsyncWrite+AsyncRead+'static
{
    pub inner:T
}

impl<W> io::Read for CryptoStream<W>
    where W:AsyncWrite+AsyncRead+'static
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        dbg!(self.inner.read(buf))
    }
}

impl<W> AsyncRead for CryptoStream<W> where W:AsyncWrite+AsyncRead+'static {}

impl<W> io::Write for CryptoStream<W>
    where W:AsyncWrite+AsyncRead+'static
{
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush()
    }
}

impl<W> AsyncWrite for CryptoStream<W> where W:AsyncWrite+AsyncRead+'static {
    fn shutdown(&mut self) -> Result<futures::Async<()>, io::Error> {
        self.inner.shutdown()
    }
}

