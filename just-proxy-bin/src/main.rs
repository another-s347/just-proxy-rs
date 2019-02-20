extern crate structopt;
#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;

mod opt;

use slog::*;
use just_proxy_lib::{opt as lib_opt,connector,run_server,run_client};
use structopt::StructOpt;

fn main(){
    let opt:opt::BinOpt = opt::BinOpt::from_args();
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::CompactFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = slog::Logger::root(drain, o!("version" => "0.5"));

    if opt.dump_config {
        opt::BinOpt::dump_config();
        return;
    }

    if opt.server {
        run_server(opt.to_server_config(),log)
    }
    else {
        run_client(opt.to_client_config(),log)
    }
}
