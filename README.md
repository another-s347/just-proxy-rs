# Just Proxy #
## A simple socks5 proxy written in Rust ##


```
USAGE:
    just-proxy-bin [FLAGS] [OPTIONS]

FLAGS:
    -d, --daemon         
        --dump-config    
    -h, --help           Prints help information
    -r, --retry          
    -s, --server         
    -V, --version        Prints version information
    -v, --verbose        

OPTIONS:
    -c, --config <config_path>                [default: /etc/justproxy/config.json]
        --protocol <protocol>                 [default: tcp]
    -S, --proxy-host <proxy_host>             [default: 127.0.0.1]
    -P, --proxy-port <proxy_port>             [default: 12346]
        --retry-interval <retry_interval>     [default: 5]
    -l, --socks-host <socks_host>             [default: 127.0.0.1]
    -p, --socks-port <socks_port>             [default: 12345]
```

Server:
```
./target/debug/just-proxy-bin -s -S 0.0.0.0 --protocol quic
```
Client:
```
./target/debug/just-proxy-bin -S YOUR_SERVER -l 0.0.0.0
```

Config file example:
```
{
  // Configuration for crypto
  "crypto_method": "aes-256-gcm",
  "salt": "2121",
  "key": "12312",
  "digest_iteration": 1,
  "digest_method": "sha256",
  // Configuration for quic, not used by tcp
  "delayed_ack_timeout": null,
  "idle_timeout": null,
  "initial_rtt": null,
  "initial_window": null,
  "local_cid_len": null,
  "loss_reduction_factor": null,
  "max_datagram_size": null,
  "max_tlps": null,
  "minimum_window": null,
  "packet_threshold": null,
  "presistent_cognestion_threshold": null,
  "receive_window": null,
  "stream_receive_window": null,
  "stream_window_bidi": null,
  "time_threshold": null
}
```