use std::mem::replace;
use futures::*;
use futures::sink;
use tokio_io::{AsyncRead, AsyncWrite};
use xml;
use sasl::common::Credentials;
use sasl::common::scram::*;
use sasl::client::Mechanism;
use sasl::client::mechanisms::*;
use serialize::base64::{self, ToBase64, FromBase64};

use xmpp_codec::*;
use xmpp_stream::*;
use stream_start::*;

const NS_XMPP_SASL: &str = "urn:ietf:params:xml:ns:xmpp-sasl";

pub struct ClientAuth<S: AsyncRead + AsyncWrite> {
    state: ClientAuthState<S>,
    mechanism: Box<Mechanism>,
}

enum ClientAuthState<S: AsyncRead + AsyncWrite> {
    WaitSend(sink::Send<XMPPStream<S>>),
    WaitRecv(XMPPStream<S>),
    Start(StreamStart<S>),
    Invalid,
}

impl<S: AsyncRead + AsyncWrite> ClientAuth<S> {
    pub fn new(stream: XMPPStream<S>, creds: Credentials) -> Result<Self, String> {
        let mechs: Vec<Box<Mechanism>> = vec![
            Box::new(Scram::<Sha256>::from_credentials(creds.clone()).unwrap()),
            Box::new(Scram::<Sha1>::from_credentials(creds.clone()).unwrap()),
            Box::new(Plain::from_credentials(creds).unwrap()),
            Box::new(Anonymous::new()),
        ];

        println!("stream_features: {}", stream.stream_features);
        let mech_names: Vec<String> =
            match stream.stream_features.get_child("mechanisms", Some(NS_XMPP_SASL)) {
                None =>
                    return Err("No auth mechanisms".to_owned()),
                Some(mechs) =>
                    mechs.get_children("mechanism", Some(NS_XMPP_SASL))
                    .map(|mech_el| mech_el.content_str())
                    .collect(),
            };
        println!("Offered mechanisms: {:?}", mech_names);

        for mut mech in mechs {
            let name = mech.name().to_owned();
            if mech_names.iter().any(|name1| *name1 == name) {
                println!("Selected mechanism: {:?}", name);
                let initial = try!(mech.initial());
                let mut this = ClientAuth {
                    state: ClientAuthState::Invalid,
                    mechanism: mech,
                };
                this.send(
                    stream,
                    "auth", &[("mechanism".to_owned(), name)],
                    &initial
                );
                return Ok(this);
            }
        }

        Err("No supported SASL mechanism available".to_owned())
    }

    fn send(&mut self, stream: XMPPStream<S>, nonza_name: &str, attrs: &[(String, String)], content: &[u8]) {
        let mut nonza = xml::Element::new(
            nonza_name.to_owned(),
            Some(NS_XMPP_SASL.to_owned()),
            attrs.iter()
                .map(|&(ref name, ref value)| (name.clone(), None, value.clone()))
                .collect()
        );
        nonza.text(content.to_base64(base64::URL_SAFE));

        println!("send {}", nonza);
        let send = stream.send(Packet::Stanza(nonza));

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
                        println!("send done");
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
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.name == "challenge"
                        && stanza.ns == Some(NS_XMPP_SASL.to_owned()) =>
                    {
                        let content = try!(
                            stanza.content_str()
                                .from_base64()
                                .map_err(|e| format!("{}", e))
                        );
                        let response = try!(self.mechanism.response(&content));
                        self.send(stream, "response", &[], &response);
                        self.poll()
                    },
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.name == "success"
                        && stanza.ns == Some(NS_XMPP_SASL.to_owned()) =>
                    {
                        let start = stream.restart();
                        self.state = ClientAuthState::Start(start);
                        self.poll()
                    },
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.name == "failure"
                        && stanza.ns == Some(NS_XMPP_SASL.to_owned()) =>
                    {
                        let mut e = None;
                        for child in &stanza.children {
                            match child {
                                &xml::Xml::ElementNode(ref child) => {
                                    e = Some(child.name.clone());
                                    break
                                },
                                _ => (),
                            }
                        }
                        let e = e.unwrap_or_else(|| "Authentication failure".to_owned());
                        Err(e)
                    },
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
