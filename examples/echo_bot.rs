extern crate futures;
extern crate tokio_core;
extern crate tokio_xmpp;
extern crate jid;
extern crate xml;

use std::str::FromStr;
use tokio_core::reactor::Core;
use futures::{Future, Stream, Sink, future};
use tokio_xmpp::{Client, ClientEvent};
use tokio_xmpp::xmpp_codec::Packet;

fn main() {
    let mut core = Core::new().unwrap();
    let client = Client::new("astrobot@example.org", "", &core.handle()).unwrap();
    // let client = TcpClient::connect(
    //     jid.clone(),
    //     &addr,
    //     &core.handle()
    // ).map_err(|e| format!("{}", e)
    // ).and_then(|stream| {
    //     if stream.can_starttls() {
    //         stream.starttls()
    //     } else {
    //         panic!("No STARTTLS")
    //     }
    // }).and_then(|stream| {
    //     let username = jid.node.as_ref().unwrap().to_owned();
    //     stream.auth(username, password).expect("auth")
    // }).and_then(|stream| {
    //     stream.bind()
    // }).and_then(|stream| {
    //     println!("Bound to {}", stream.jid);

    //     let presence = xml::Element::new("presence".to_owned(), None, vec![]);
    //     stream.send(Packet::Stanza(presence))
    //         .map_err(|e| format!("{}", e))
    // }).and_then(|stream| {
    //     let main_loop = |stream| {
    //         stream.into_future()
    //             .and_then(|(event, stream)| {
    //                 stream.send(Packet::Stanza(unreachable!()))
    //             }).and_then(main_loop)
    //     };
    //     main_loop(stream)
    // }).and_then(|(event, stream)| {
    //     let (mut sink, stream) = stream.split();
    //     stream.for_each(move |event| {
    //         match event {
    //             Packet::Stanza(ref message)
    //                 if message.name == "message" => {
    //                     let ty = message.get_attribute("type", None);
    //                     let body = message.get_child("body", Some("jabber:client"))
    //                         .map(|body_el| body_el.content_str());
    //                     match ty {
    //                         None | Some("normal") | Some("chat")
    //                             if body.is_some() => {
    //                                 let from = message.get_attribute("from", None).unwrap();
    //                                 println!("Got message from {}: {:?}", from, body);
    //                                 let reply = make_reply(from, body.unwrap());
    //                                 sink.send(Packet::Stanza(reply))
    //                                     .and_then(|_| Ok(()))
    //                             },
    //                         _ => future::ok(()),
    //                     }
    //                 },
    //             _ => future::ok(()),
    //         }
    //     }).map_err(|e| format!("{}", e))
    // });

    let done = client.for_each(|event| {
        match event {
            ClientEvent::Online => {
                println!("Online!");
            },
            ClientEvent::Stanza(stanza) => {
            },
            _ => {
                println!("Event: {:?}", event);
            },
        }
        
        Ok(())
    });
    
    match core.run(done) {
        Ok(_) => (),
        Err(e) => {
            println!("Fatal: {}", e);
            ()
        }
    }
}

fn make_reply(to: &str, body: String) -> xml::Element {
    let mut message = xml::Element::new(
        "message".to_owned(),
        None,
        vec![("type".to_owned(), None, "chat".to_owned()),
             ("to".to_owned(), None, to.to_owned())]
    );
    message.tag(xml::Element::new("body".to_owned(), None, vec![]))
        .text(body);
    message
}
