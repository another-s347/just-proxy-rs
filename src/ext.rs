use futures::prelude::*;

pub struct OnlyFirstStream<T,I>
    where T:Stream<Item=I,Error=()>
{
    pub inner:T,
    pub first_taken:bool
}

impl<T,I> Stream for OnlyFirstStream<T,I>
    where T:Stream<Item=I,Error=()>
{
    type Item = I;
    type Error = ();

    fn poll(&mut self) -> Result<futures::Async<Option<Self::Item>>, Self::Error> {
        match (self.inner.poll(),self.first_taken) {
            (_,true)=>{
                Ok(futures::Async::Ready(None))
            }
            (Ok(futures::Async::Ready(i)),false)=>{
                self.first_taken=true;
                Ok(futures::Async::Ready(i))
            }
            (other,_)=>{
                other
            }
        }
    }
}