extern crate futures;
extern crate tokio_core;
extern crate tokio_xmpp;
extern crate rustls;

use std::sync::Arc;
use std::io::BufReader;
use std::fs::File;
use tokio_core::reactor::Core;
use futures::{Future, Stream};
use tokio_xmpp::TcpClient;
use tokio_xmpp::xmpp_codec::Packet;
use rustls::ClientConfig;

fn main() {
    use std::net::ToSocketAddrs;
    let addr = "[2a01:4f8:a0:33d0::5]:5222"
        .to_socket_addrs().unwrap()
        .next().unwrap();

    let mut config = ClientConfig::new();
    let mut certfile = BufReader::new(File::open("/usr/share/ca-certificates/CAcert/root.crt").unwrap());
    config.root_store.add_pem_file(&mut certfile).unwrap();
    let arc_config = Arc::new(config);

    let mut core = Core::new().unwrap();
    let client = TcpClient::connect(
        &addr,
        &core.handle()
    ).and_then(|stream| {
        if stream.can_starttls() {
            stream.starttls(arc_config)
        } else {
            panic!("No STARTTLS")
        }
    }).and_then(|stream| {
        stream.for_each(|event| {
            match event {
                Packet::Stanza(el) => println!("<< {}", el),
                _ => println!("!! {:?}", event),
            }
            Ok(())
        })
    });
    match core.run(client) {
        Ok(_) => (),
        Err(e) => {
            println!("Fatal: {}", e);
            ()
        }
    }
}
