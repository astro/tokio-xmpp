extern crate futures;
extern crate tokio_core;
extern crate tokio_xmpp;
extern crate jid;
extern crate xml;

use tokio_core::reactor::Core;
use futures::{Future, Stream, Sink, future};
use tokio_xmpp::Client;

fn main() {
    let mut core = Core::new().unwrap();
    let client = Client::new("astrobot@example.org", "", &core.handle()).unwrap();

    let (sink, stream) = client.split();
    let mut sink = Some(sink);
    let mut send = move |stanza| {
        sink = Some(
            sink.take().
                expect("sink")
                .send(stanza)
                .wait()
                .expect("sink.send")
        );
    };
    let done = stream.for_each(|event| {
        let result: Box<Future<Item=(), Error=String>> =
            if event.is_online() {
                println!("Online!");

                let presence = make_presence();
                send(presence);
                Box::new(
                    future::ok(())
                )
            } else if let Some(stanza) = event.as_stanza() {
                if stanza.name == "message" &&
                    stanza.get_attribute("type", None) != Some("error") {
                        let from = stanza.get_attribute("from", None);
                        let body = stanza.get_child("body", Some("jabber:client"))
                            .map(|el| el.content_str());

                        match (from.as_ref(), body) {
                            (Some(from), Some(body)) => {
                                let reply = make_reply(from, body);
                                send(reply);
                            },
                            _ => (),
                        };
                    }
                Box::new(future::ok(()))
            } else {
                Box::new(future::ok(()))
            };
        result
    });
    
    match core.run(done) {
        Ok(_) => (),
        Err(e) => {
            println!("Fatal: {}", e);
            ()
        }
    }
}

fn make_presence() -> xml::Element {
    let mut presence = xml::Element::new("presence".to_owned(), None, vec![]);
    presence.tag(xml::Element::new("status".to_owned(), None, vec![]))
        .text("chat".to_owned());
    presence.tag(xml::Element::new("show".to_owned(), None, vec![]))
        .text("Echoing messages".to_owned());
    presence
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
