use futures::{future, Sink, Stream};
use jid::Jid;
use minidom::Element;
use std::env::args;
use std::process::exit;
use tokio::runtime::current_thread::Runtime;
use tokio_xmpp::Client;
use try_from::TryFrom;
use xmpp_parsers::message::{Body, Message, MessageType};
use xmpp_parsers::presence::{Presence, Show as PresenceShow, Type as PresenceType};

fn main() {
    let args: Vec<String> = args().collect();
    if args.len() != 3 {
        println!("Usage: {} <jid> <password>", args[0]);
        exit(1);
    }
    let jid = &args[1];
    let password = &args[2];

    // tokio_core context
    let mut rt = Runtime::new().unwrap();
    // Client instance
    let client = Client::new(jid, password).unwrap();

    // Make the two interfaces for sending and receiving independent
    // of each other so we can move one into a closure.
    let (mut sink, stream) = client.split();
    // Wrap sink in Option so that we can take() it for the send(self)
    // to consume and return it back when ready.
    let mut send = move |stanza| {
        sink.start_send(stanza).expect("start_send");
    };
    // Main loop, processes events
    let done = stream.for_each(|event| {
        if event.is_online() {
            println!("Online!");

            let presence = make_presence();
            send(presence);
        } else if let Some(message) = event
            .into_stanza()
            .and_then(|stanza| Message::try_from(stanza).ok())
        {
            // This is a message we'll echo
            match (message.from, message.bodies.get("")) {
                (Some(from), Some(body)) => {
                    if message.type_ != MessageType::Error {
                        let reply = make_reply(from, &body.0);
                        send(reply);
                    }
                }
                _ => (),
            }
        }

        Box::new(future::ok(()))
    });

    // Start polling `done`
    match rt.block_on(done) {
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
    presence
        .statuses
        .insert(String::from("en"), String::from("Echoing messages."));
    presence.into()
}

// Construct a chat <message/>
fn make_reply(to: Jid, body: &str) -> Element {
    let mut message = Message::new(Some(to));
    message.bodies.insert(String::new(), Body(body.to_owned()));
    message.into()
}
