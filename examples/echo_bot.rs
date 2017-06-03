extern crate futures;
extern crate tokio_core;
extern crate tokio_xmpp;

use tokio_core::reactor::Core;
use futures::{Future, Stream};
use tokio_xmpp::{Packet, TcpClient};

fn main() {
    use std::net::ToSocketAddrs;
    let addr = "[2a01:4f8:a0:33d0::5]:5222"
        .to_socket_addrs().unwrap()
        .next().unwrap();

    let mut core = Core::new().unwrap();
    let client = TcpClient::connect(
        &addr,
        &core.handle()
    ).and_then(|stream| {
        stream.for_each(|event| {
            match event {
                Packet::Stanza(el) => println!("<< {}", el),
                _ => println!("!! {:?}", event),
            }
            Ok(())
        })
    });
    core.run(client).unwrap();
}
