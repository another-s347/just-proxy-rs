#[macro_use]
extern crate failure;
#[macro_use]
extern crate slog;

use slog::Drain;
use structopt::StructOpt;

pub mod component;
pub mod message;
pub mod connector;
pub mod ext;
pub mod opt;

//pub struct ProxyServer<W> where W: AsyncWrite + 'static {
//    clients: Vec<Addr<ProxyClient<W>>>,
//    logger:slog::Logger
//}
//
//impl<W> Actor for ProxyServer<W>
//    where W: AsyncWrite + 'static
//{
//    type Context = Context<Self>;
//}
//
//impl<W> Handler<connector::ConnectorMessage<W>> for ProxyServer<W>
//    where W: AsyncWrite+AsyncRead + 'static
//{
//    type Result = ();
//
//    fn handle(&mut self, msg: connector::ConnectorMessage<W>, ctx: &mut Self::Context) -> Self::Result {
//        let (r, w) = msg.connector.split();
//        let proxy_client_logger = self.logger.new(o!("client"=>""));
//        let addr = ProxyClient::create(move |ctx| {
//            ProxyClient::add_stream(FramedRead::new(r, ActorMessage::ProxyRequestCodec), ctx);
//            let writer = actix::io::FramedWrite::new(w, ActorMessage::ProxyResponseCodec, ctx);
//            ProxyClient {
//                writer,
//                connections: HashMap::new(),
//                logger:proxy_client_logger
//            }
//        });
//        self.clients.push(addr);
//    }
//}

//fn listen_callback<W>(logger:slog::Logger)->impl FnOnce(Box<Stream<Item=W, Error=()>>)
//    where W:AsyncRead+AsyncWrite+'static
//{
//    move|s|{
//        ProxyServer::create(|ctx| {
//            ctx.add_message_stream(s.map(|st| {
//                connector::ConnectorMessage {
//                    connector: st
//                }
//            }));
//            ProxyServer {
//                clients: Vec::new(),
//                logger
//            }
//        });
//    }
//}

fn main() {
    let opt: opt::ServerOpt = dbg!(opt::ServerOpt::from_args());

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let log = slog::Logger::root(drain, o!("version" => "0.5"));

    actix::System::run(move || {
        match opt.protocol.as_str() {
            "tcp" => {
                let connector = connector::tcp::TcpConnector{};
                connector.run_server(&format!("{}:{}", opt.proxy_host, opt.proxy_port),log);
            },
            "quic" => {
                let connector=connector::quic::QuicServerConnector::new_dangerous();
                connector.run_server(&format!("{}:{}", opt.proxy_host, opt.proxy_port),log);
            },
            _ => panic!("unsupported protocol")
        };
    });
}