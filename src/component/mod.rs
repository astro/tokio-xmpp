//! Components in XMPP are services/gateways that are logged into an
//! XMPP server under a JID consisting of just a domain name. They are
//! allowed to use any user and resource identifiers in their stanzas.
use std::mem::replace;
use std::str::FromStr;
use std::error::Error;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use futures::{Future, Stream, Poll, Async, Sink, StartSend, AsyncSink};
use minidom::Element;
use jid::{Jid, JidParseError};

use super::xmpp_codec::Packet;
use super::xmpp_stream;
use super::happy_eyeballs::Connecter;
use super::event::Event;

mod auth;
use self::auth::ComponentAuth;

/// Component connection to an XMPP server
pub struct Component {
    /// The component's Jabber-Id
    pub jid: Jid,
    state: ComponentState,
}

type XMPPStream = xmpp_stream::XMPPStream<TcpStream>;
const NS_JABBER_COMPONENT_ACCEPT: &str = "jabber:component:accept";

enum ComponentState {
    Invalid,
    Disconnected,
    Connecting(Box<Future<Item=XMPPStream, Error=String>>),
    Connected(XMPPStream),
}

impl Component {
    /// Start a new XMPP component
    ///
    /// Start polling the returned instance so that it will connect
    /// and yield events.
    pub fn new(jid: &str, password: &str, server: &str, port: u16, handle: Handle) -> Result<Self, JidParseError> {
        let jid = Jid::from_str(jid)?;
        let password = password.to_owned();
        let connect = Self::make_connect(jid.clone(), password, server, port, handle);
        Ok(Component {
            jid,
            state: ComponentState::Connecting(connect),
        })
    }

    fn make_connect(jid: Jid, password: String, server: &str, port: u16, handle: Handle) -> Box<Future<Item=XMPPStream, Error=String>> {
        let jid1 = jid.clone();
        let password = password;
        Box::new(
            Connecter::from_lookup(handle, server, "_xmpp-component._tcp", port)
                .expect("Connector::from_lookup")
                .and_then(move |tcp_stream| {
                    xmpp_stream::XMPPStream::start(tcp_stream, jid1, NS_JABBER_COMPONENT_ACCEPT.to_owned())
                    .map_err(|e| format!("{}", e))
                }).and_then(move |xmpp_stream| {
                    Self::auth(xmpp_stream, password).expect("auth")
                }).and_then(|xmpp_stream| {
                    // println!("Bound to {}", xmpp_stream.jid);
                    Ok(xmpp_stream)
                })
        )
    }

    fn auth<S: AsyncRead + AsyncWrite>(stream: xmpp_stream::XMPPStream<S>, password: String) -> Result<ComponentAuth<S>, String> {
        ComponentAuth::new(stream, password)
    }
}

impl Stream for Component {
    type Item = Event;
    type Error = String;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let state = replace(&mut self.state, ComponentState::Invalid);

        match state {
            ComponentState::Invalid =>
                Err("invalid client state".to_owned()),
            ComponentState::Disconnected =>
                Ok(Async::Ready(None)),
            ComponentState::Connecting(mut connect) => {
                match connect.poll() {
                    Ok(Async::Ready(stream)) => {
                        self.state = ComponentState::Connected(stream);
                        Ok(Async::Ready(Some(Event::Online)))
                    },
                    Ok(Async::NotReady) => {
                        self.state = ComponentState::Connecting(connect);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(e),
                }
            },
            ComponentState::Connected(mut stream) => {
                // Poll sink
                match stream.poll_complete() {
                    Ok(Async::NotReady) => (),
                    Ok(Async::Ready(())) => (),
                    Err(e) =>
                        return Err(e.description().to_owned()),
                };

                // Poll stream
                match stream.poll() {
                    Ok(Async::NotReady) => {
                        self.state = ComponentState::Connected(stream);
                        Ok(Async::NotReady)
                    },
                    Ok(Async::Ready(None)) => {
                        // EOF
                        self.state = ComponentState::Disconnected;
                        Ok(Async::Ready(Some(Event::Disconnected)))
                    },
                    Ok(Async::Ready(Some(Packet::Stanza(stanza)))) => {
                        self.state = ComponentState::Connected(stream);
                        Ok(Async::Ready(Some(Event::Stanza(stanza))))
                    },
                    Ok(Async::Ready(_)) => {
                        self.state = ComponentState::Connected(stream);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(e.description().to_owned()),
                }
            },
        }
    }
}

impl Sink for Component {
    type SinkItem = Element;
    type SinkError = String;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.state {
            ComponentState::Connected(ref mut stream) =>
                match stream.start_send(Packet::Stanza(item)) {
                    Ok(AsyncSink::NotReady(Packet::Stanza(stanza))) =>
                        Ok(AsyncSink::NotReady(stanza)),
                    Ok(AsyncSink::NotReady(_)) =>
                        panic!("Component.start_send with stanza but got something else back"),
                    Ok(AsyncSink::Ready) => {
                        Ok(AsyncSink::Ready)
                    },
                    Err(e) =>
                        Err(e.description().to_owned()),
                },
            _ =>
                Ok(AsyncSink::NotReady(item)),
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        match &mut self.state {
            &mut ComponentState::Connected(ref mut stream) =>
                stream.poll_complete()
                .map_err(|e| e.description().to_owned()),
            _ =>
                Ok(Async::Ready(())),
        }
    }
}
