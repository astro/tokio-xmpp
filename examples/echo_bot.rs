extern crate futures;
extern crate tokio_core;
extern crate tokio_xmpp;
extern crate jid;
extern crate xml;

use std::str::FromStr;
use tokio_core::reactor::Core;
use futures::{Future, Stream, Sink};
use jid::Jid;
use tokio_xmpp::TcpClient;
use tokio_xmpp::xmpp_codec::Packet;

fn main() {
    let jid = Jid::from_str("astrobot@example.net").expect("JID");
    let password = "".to_owned();

    use std::net::ToSocketAddrs;
    let addr = "[2a01:4f8:a0:33d0::5]:5222"
        .to_socket_addrs().unwrap()
        .next().unwrap();

    let mut core = Core::new().unwrap();
    let client = TcpClient::connect(
        jid.clone(),
        &addr,
        &core.handle()
    ).map_err(|e| format!("{}", e)
    ).and_then(|stream| {
        if stream.can_starttls() {
            stream.starttls()
        } else {
            panic!("No STARTTLS")
        }
    }).and_then(|stream| {
        let username = jid.node.as_ref().unwrap().to_owned();
        stream.auth(username, password).expect("auth")
    }).and_then(|stream| {
        stream.bind()
    }).and_then(|stream| {
        println!("Bound to {}", stream.jid);

        let presence = xml::Element::new("presence".to_owned(), None, vec![]);
        stream.send(Packet::Stanza(presence))
            .map_err(|e| format!("{}", e))
    }).and_then(|stream| {
        stream.for_each(|event| {
            match event {
                Packet::Stanza(el) => println!("<< {}", el),
                _ => println!("!! {:?}", event),
            }
            Ok(())
        }).map_err(|e| format!("{}", e))
    });
    match core.run(client) {
        Ok(_) => (),
        Err(e) => {
            println!("Fatal: {}", e);
            ()
        }
    }
}
