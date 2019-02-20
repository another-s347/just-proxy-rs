extern crate structopt;
#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_term;

mod opt;

use slog::*;
use just_proxy_lib::{opt as lib_opt,connector,run_server,run_client};
use structopt::StructOpt;
use std::fs::File;
use daemonize::Daemonize;

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

    if opt.daemon {
        let name = if opt.server {
            "/tmp/justproxy-server"
        } else {
            "/tmp/justproxy-client"
        };
        let stdout = File::create(format!("{}.out",name)).unwrap();
        let stderr = File::create(format!("{}.err",name)).unwrap();

        let daemonize = Daemonize::new()
            .pid_file(format!("{}.pid",name)) // Every method except `new` and `start`
            .stdout(stdout)  // Redirect stdout to `/tmp/daemon.out`.
            .stderr(stderr);  // Redirect stderr to `/tmp/daemon.err`.

        match daemonize.start() {
            Ok(_) => {
                run(opt,log)
            },
            Err(e) => eprintln!("Error, {}", e),
        }
    }
    else{
        run(opt,log)
    }
}

fn run(opt:opt::BinOpt,log:Logger){
    if opt.server {
        run_server(opt.to_server_config(),log)
    }
    else {
        if opt.retry {
            loop {
                run_client(opt.to_client_config(),log.clone());
                std::thread::sleep(std::time::Duration::from_secs(opt.retry_interval))
            }
        }
        else {
            run_client(opt.to_client_config(),log.clone());
        }
    }
}
