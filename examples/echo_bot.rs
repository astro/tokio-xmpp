extern crate futures;
extern crate tokio_core;
extern crate tokio_xmpp;
extern crate jid;
extern crate minidom;
extern crate xmpp_parsers;
extern crate try_from;

use std::env::args;
use std::process::exit;
use try_from::TryFrom;
use tokio_core::reactor::Core;
use futures::{Future, Stream, Sink, future};
use tokio_xmpp::Client;
use minidom::Element;
use xmpp_parsers::presence::{Presence, Type as PresenceType, Show as PresenceShow};
use xmpp_parsers::message::{Message, MessageType, Body};
use jid::Jid;

fn main() {
    let args: Vec<String> = args().collect();
    if args.len() != 3 {
        println!("Usage: {} <jid> <password>", args[0]);
        exit(1);
    }
    let jid = &args[1];
    let password = &args[2];

    // tokio_core context
    let mut core = Core::new().unwrap();
    // Client instance
    let client = Client::new(jid, password, core.handle()).unwrap();

    // Make the two interfaces for sending and receiving independent
    // of each other so we can move one into a closure.
    let (sink, stream) = client.split();
    // Wrap sink in Option so that we can take() it for the send(self)
    // to consume and return it back when ready.
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
    // Main loop, processes events
    let done = stream.for_each(|event| {
        if event.is_online() {
            println!("Online!");

            let presence = make_presence();
            send(presence);
        } else if let Some(message) = event.into_stanza()
            .and_then(|stanza| Message::try_from(stanza).ok())
        {
            // This is a message we'll echo
            match (message.from, message.bodies.get("")) {
                (Some(from), Some(body)) =>
                    if message.type_ != MessageType::Error {
                        let reply = make_reply(from, &body.0);
                        send(reply);
                    },
                _ => (),
            }
        }

        Box::new(future::ok(()))
    });

    // Start polling `done`
    match core.run(done) {
        Ok(_) => (),
        Err(e) => {
            println!("Fatal: {}", e);
            ()
        }
    }
}

// Construct a <presence/>
fn make_presence() -> Element {
    let mut presence = Presence::new(PresenceType::None);
    presence.show = PresenceShow::Chat;
    presence.statuses.insert(String::from("en"), String::from("Echoing messages."));
    presence.into()
}

// Construct a chat <message/>
fn make_reply(to: Jid, body: &str) -> Element {
    let mut message = Message::new(Some(to));
    message.bodies.insert(String::new(), Body(body.to_owned()));
    message.into()
}
