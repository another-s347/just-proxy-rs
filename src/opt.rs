use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "client")]
pub struct ClientOpt {

    // The number of occurrences of the `v/verbose` flag
    /// Verbose mode (-v, -vv, -vvv, etc.)
    #[structopt(short = "v", long = "verbose")]
    pub verbose: bool,

    #[structopt(short = "l", long = "socks-host", default_value = "127.0.0.1")]
    pub socks_host:String,

    #[structopt(short = "S", long = "proxy-host", default_value = "127.0.0.1")]
    pub proxy_host: String,

    #[structopt(short = "p", long="socks-port", default_value = "12345")]
    pub socks_port: i64,

    #[structopt(short = "P", long = "proxy-port", default_value="12346")]
    pub proxy_port: i64,

    #[structopt(long="protocol", default_value="tcp")]
    pub protocol:String
}

#[derive(StructOpt, Debug)]
#[structopt(name = "server")]
pub struct ServerOpt {

    // The number of occurrences of the `v/verbose` flag
    /// Verbose mode (-v, -vv, -vvv, etc.)
    #[structopt(short = "v", long = "verbose")]
    pub verbose: bool,

    #[structopt(short = "l", long = "proxy-host", default_value = "127.0.0.1")]
    pub proxy_host: String,

    #[structopt(short = "p", long = "proxy-port", default_value="12346")]
    pub proxy_port: i64,

    #[structopt(long="protocol", default_value="tcp")]
    pub protocol:String
}