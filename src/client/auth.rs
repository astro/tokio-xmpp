use std::mem::replace;
use futures::{Future, Poll, Async, sink, Stream};
use tokio_io::{AsyncRead, AsyncWrite};
use sasl::common::Credentials;
use sasl::common::scram::{Sha1, Sha256};
use sasl::client::Mechanism;
use sasl::client::mechanisms::{Scram, Plain, Anonymous};
use minidom::Element;
use xmpp_parsers::sasl::{Auth, Challenge, Response, Success, Failure, Mechanism as XMPPMechanism};
use try_from::TryFrom;

use xmpp_codec::Packet;
use xmpp_stream::XMPPStream;
use stream_start::StreamStart;

const NS_XMPP_SASL: &str = "urn:ietf:params:xml:ns:xmpp-sasl";

pub struct ClientAuth<S: AsyncWrite> {
    state: ClientAuthState<S>,
    mechanism: Box<Mechanism>,
}

enum ClientAuthState<S: AsyncWrite> {
    WaitSend(sink::Send<XMPPStream<S>>),
    WaitRecv(XMPPStream<S>),
    Start(StreamStart<S>),
    Invalid,
}

impl<S: AsyncWrite> ClientAuth<S> {
    pub fn new(stream: XMPPStream<S>, creds: Credentials) -> Result<Self, String> {
        let mechs: Vec<(Box<Mechanism>, XMPPMechanism)> = vec![
            (Box::new(Scram::<Sha256>::from_credentials(creds.clone()).unwrap()),
             XMPPMechanism::ScramSha256
            ),
            (Box::new(Scram::<Sha1>::from_credentials(creds.clone()).unwrap()),
             XMPPMechanism::ScramSha1
            ),
            (Box::new(Plain::from_credentials(creds).unwrap()),
             XMPPMechanism::Plain
            ),
            (Box::new(Anonymous::new()),
             XMPPMechanism::Anonymous
            ),
        ];

        let mech_names: Vec<String> =
            match stream.stream_features.get_child("mechanisms", NS_XMPP_SASL) {
                None =>
                    return Err("No auth mechanisms".to_owned()),
                Some(mechs) =>
                    mechs.children()
                    .filter(|child| child.is("mechanism", NS_XMPP_SASL))
                    .map(|mech_el| mech_el.text())
                    .collect(),
            };
        println!("SASL mechanisms offered: {:?}", mech_names);

        for (mut mech, mechanism) in mechs {
            let name = mech.name().to_owned();
            if mech_names.iter().any(|name1| *name1 == name) {
                println!("SASL mechanism selected: {:?}", name);
                let initial = try!(mech.initial());
                let mut this = ClientAuth {
                    state: ClientAuthState::Invalid,
                    mechanism: mech,
                };
                this.send(
                    stream,
                    Auth {
                        mechanism,
                        data: initial,
                    }
                );
                return Ok(this);
            }
        }

        Err("No supported SASL mechanism available".to_owned())
    }

    fn send<N: Into<Element>>(&mut self, stream: XMPPStream<S>, nonza: N) {
        let send = stream.send_stanza(nonza);

        self.state = ClientAuthState::WaitSend(send);
    }
}

impl<S: AsyncRead + AsyncWrite> Future for ClientAuth<S> {
    type Item = XMPPStream<S>;
    type Error = String;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let state = replace(&mut self.state, ClientAuthState::Invalid);

        match state {
            ClientAuthState::WaitSend(mut send) =>
                match send.poll() {
                    Ok(Async::Ready(stream)) => {
                        self.state = ClientAuthState::WaitRecv(stream);
                        self.poll()
                    },
                    Ok(Async::NotReady) => {
                        self.state = ClientAuthState::WaitSend(send);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(format!("{}", e)),
                },
            ClientAuthState::WaitRecv(mut stream) =>
                match stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(stanza)))) => {
                        if let Ok(challenge) = Challenge::try_from(stanza.clone()) {
                            let response = try!(self.mechanism.response(&challenge.data));
                            self.send(stream, Response { data: response });
                            self.poll()
                        } else if let Ok(_) = Success::try_from(stanza.clone()) {
                            let start = stream.restart();
                            self.state = ClientAuthState::Start(start);
                            self.poll()
                        } else if let Ok(failure) = Failure::try_from(stanza) {
                            let e = failure.data;
                            Err(e)
                        } else {
                            Ok(Async::NotReady)
                        }
                    }
                    Ok(Async::Ready(event)) => {
                        println!("ClientAuth ignore {:?}", event);
                        Ok(Async::NotReady)
                    },
                    Ok(_) => {
                        self.state = ClientAuthState::WaitRecv(stream);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(format!("{}", e)),
                },
            ClientAuthState::Start(mut start) =>
                match start.poll() {
                    Ok(Async::Ready(stream)) =>
                        Ok(Async::Ready(stream)),
                    Ok(Async::NotReady) => {
                        self.state = ClientAuthState::Start(start);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(format!("{}", e)),
                },
            ClientAuthState::Invalid =>
                unreachable!(),
        }
    }
}
