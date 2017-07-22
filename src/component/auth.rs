use std::mem::replace;
use futures::{Future, Poll, Async, sink, Sink, Stream};
use tokio_io::{AsyncRead, AsyncWrite};
use minidom::Element;
use sha_1::{Sha1, Digest};

use xmpp_codec::Packet;
use xmpp_stream::XMPPStream;

const NS_JABBER_COMPONENT_ACCEPT: &str = "jabber:component:accept";

pub struct ComponentAuth<S: AsyncWrite> {
    state: ComponentAuthState<S>,
}

enum ComponentAuthState<S: AsyncWrite> {
    WaitSend(sink::Send<XMPPStream<S>>),
    WaitRecv(XMPPStream<S>),
    Invalid,
}

impl<S: AsyncWrite> ComponentAuth<S> {
    pub fn new(stream: XMPPStream<S>, password: String) -> Result<Self, String> {
        // FIXME: huge hack, shouldn’t be an element!
        let sid = stream.stream_features.name().to_owned();
        let mut this = ComponentAuth {
            state: ComponentAuthState::Invalid,
        };
        this.send(
            stream,
            "handshake",
            // TODO: sha1(sid + password)
            &format!("{:x}", Sha1::digest((sid + &password).as_bytes()))
        );
        return Ok(this);
    }

    fn send(&mut self, stream: XMPPStream<S>, nonza_name: &str, handshake: &str) {
        let nonza = Element::builder(nonza_name)
            .ns(NS_JABBER_COMPONENT_ACCEPT)
            .append(handshake)
            .build();

        let send = stream.send(Packet::Stanza(nonza));

        self.state = ComponentAuthState::WaitSend(send);
    }
}

impl<S: AsyncRead + AsyncWrite> Future for ComponentAuth<S> {
    type Item = XMPPStream<S>;
    type Error = String;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let state = replace(&mut self.state, ComponentAuthState::Invalid);

        match state {
            ComponentAuthState::WaitSend(mut send) =>
                match send.poll() {
                    Ok(Async::Ready(stream)) => {
                        self.state = ComponentAuthState::WaitRecv(stream);
                        self.poll()
                    },
                    Ok(Async::NotReady) => {
                        self.state = ComponentAuthState::WaitSend(send);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(format!("{}", e)),
                },
            ComponentAuthState::WaitRecv(mut stream) =>
                match stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.name() == "handshake"
                        && stanza.ns() == Some(NS_JABBER_COMPONENT_ACCEPT) =>
                    {
                        self.state = ComponentAuthState::Invalid;
                        Ok(Async::Ready(stream))
                    },
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.is("error", "http://etherx.jabber.org/streams") =>
                    {
                        let e = "Authentication failure";
                        Err(e.to_owned())
                    },
                    Ok(Async::Ready(event)) => {
                        println!("ComponentAuth ignore {:?}", event);
                        Ok(Async::NotReady)
                    },
                    Ok(_) => {
                        self.state = ComponentAuthState::WaitRecv(stream);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(format!("{}", e)),
                },
            ComponentAuthState::Invalid =>
                unreachable!(),
        }
    }
}
