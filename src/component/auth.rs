use std::mem::replace;
use futures::{Future, Poll, Async, sink, Stream};
use tokio_io::{AsyncRead, AsyncWrite};
use xmpp_parsers::component::Handshake;

use xmpp_codec::Packet;
use xmpp_stream::XMPPStream;
use {Error, AuthError};

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
    // TODO: doesn't have to be a Result<> actually
    pub fn new(stream: XMPPStream<S>, password: String) -> Result<Self, Error> {
        // FIXME: huge hack, shouldn’t be an element!
        let sid = stream.stream_features.name().to_owned();
        let mut this = ComponentAuth {
            state: ComponentAuthState::Invalid,
        };
        this.send(
            stream,
            Handshake::from_password_and_stream_id(&password, &sid)
        );
        Ok(this)
    }

    fn send(&mut self, stream: XMPPStream<S>, handshake: Handshake) {
        let nonza = handshake;
        let send = stream.send_stanza(nonza);

        self.state = ComponentAuthState::WaitSend(send);
    }
}

impl<S: AsyncRead + AsyncWrite> Future for ComponentAuth<S> {
    type Item = XMPPStream<S>;
    type Error = Error;

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
                        Err(e)?
                },
            ComponentAuthState::WaitRecv(mut stream) =>
                match stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.is("handshake", NS_JABBER_COMPONENT_ACCEPT) =>
                    {
                        self.state = ComponentAuthState::Invalid;
                        Ok(Async::Ready(stream))
                    },
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.is("error", "http://etherx.jabber.org/streams") =>
                    {
                        Err(AuthError::ComponentFail.into())
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
                        Err(e)?
                },
            ComponentAuthState::Invalid =>
                unreachable!(),
        }
    }
}
