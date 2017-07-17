use std::mem::replace;
use std::error::Error;
use std::str::FromStr;
use futures::*;
use futures::sink;
use tokio_io::{AsyncRead, AsyncWrite};
use jid::Jid;
use minidom::Element;

use xmpp_codec::*;
use xmpp_stream::*;

const NS_XMPP_BIND: &str = "urn:ietf:params:xml:ns:xmpp-bind";
const BIND_REQ_ID: &str = "resource-bind";

pub enum ClientBind<S: AsyncWrite> {
    Unsupported(XMPPStream<S>),
    WaitSend(sink::Send<XMPPStream<S>>),
    WaitRecv(XMPPStream<S>),
    Invalid,
}

impl<S: AsyncWrite> ClientBind<S> {
    /// Consumes and returns the stream to express that you cannot use
    /// the stream for anything else until the resource binding
    /// req/resp are done.
    pub fn new(stream: XMPPStream<S>) -> Self {
        match stream.stream_features.get_child("bind", NS_XMPP_BIND) {
            None =>
                // No resource binding available,
                // return the (probably // usable) stream immediately
                ClientBind::Unsupported(stream),
            Some(_) => {
                let iq = make_bind_request(stream.jid.resource.as_ref());
                let send = stream.send(Packet::Stanza(iq));
                ClientBind::WaitSend(send)
            },
        }
    }
}

fn make_bind_request(resource: Option<&String>) -> Element {
    let iq = Element::builder("iq")
        .attr("type", "set")
        .attr("id", BIND_REQ_ID);
    let mut bind_el = Element::builder("bind")
        .ns(NS_XMPP_BIND);
    match resource {
        Some(resource) => {
            let resource_el = Element::builder("resource")
                .append(resource);
            bind_el = bind_el.append(resource_el.build());
        },
        None => (),
    }
    iq.append(bind_el.build())
        .build()
}

impl<S: AsyncRead + AsyncWrite> Future for ClientBind<S> {
    type Item = XMPPStream<S>;
    type Error = String;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let state = replace(self, ClientBind::Invalid);

        match state {
            ClientBind::Unsupported(stream) =>
                Ok(Async::Ready(stream)),
            ClientBind::WaitSend(mut send) => {
                match send.poll() {
                    Ok(Async::Ready(stream)) => {
                        replace(self, ClientBind::WaitRecv(stream));
                        self.poll()
                    },
                    Ok(Async::NotReady) => {
                        replace(self, ClientBind::WaitSend(send));
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(e.description().to_owned()),
                }
            },
            ClientBind::WaitRecv(mut stream) => {
                match stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(ref iq))))
                        if iq.name() == "iq"
                        && iq.attr("id") == Some(BIND_REQ_ID) => {
                            match iq.attr("type") {
                                Some("result") => {
                                    get_bind_response_jid(&iq)
                                        .map(|jid| stream.jid = jid);
                                    Ok(Async::Ready(stream))
                                },
                                _ =>
                                    Err("resource bind response".to_owned()),
                            }
                        },
                    Ok(Async::Ready(_)) => {
                        replace(self, ClientBind::WaitRecv(stream));
                        self.poll()
                    },
                    Ok(Async::NotReady) => {
                        replace(self, ClientBind::WaitRecv(stream));
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(e.description().to_owned()),
                }
            },
            ClientBind::Invalid =>
                unreachable!(),
        }
    }
}

fn get_bind_response_jid(iq: &Element) -> Option<Jid> {
    iq.get_child("bind", NS_XMPP_BIND)
        .and_then(|bind_el|
                  bind_el.get_child("jid", NS_XMPP_BIND)
        )
        .and_then(|jid_el|
                  Jid::from_str(&jid_el.text())
                  .ok()
        )
}
