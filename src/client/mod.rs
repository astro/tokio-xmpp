use std::mem::replace;
use std::str::FromStr;
use std::error::Error;
use std::collections::VecDeque;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tls::TlsStream;
use futures::{future, Future, Stream, Poll, Async, Sink, StartSend, AsyncSink};
use minidom::Element;
use jid::{Jid, JidParseError};
use sasl::common::{Credentials, ChannelBinding};
use idna;

use super::xmpp_codec::Packet;
use super::xmpp_stream;
use super::starttls::{NS_XMPP_TLS, StartTlsClient};
use super::happy_eyeballs::Connecter;
use super::event::Event;

mod auth;
use self::auth::ClientAuth;
mod bind;
use self::bind::ClientBind;
mod iq;

pub struct Client {
    pub jid: Jid,
    state: ClientState,
    write_queue: VecDeque<Element>,
    iq_tracker: iq::Tracker,
}

type XMPPStream = xmpp_stream::XMPPStream<TlsStream<TcpStream>>;
const NS_JABBER_CLIENT: &str = "jabber:client";

enum ClientState {
    Invalid,
    Disconnected,
    Connecting(Box<Future<Item=XMPPStream, Error=String>>),
    Connected(XMPPStream),
}

impl Client {
    pub fn new(jid: &str, password: &str, handle: Handle) -> Result<Self, JidParseError> {
        let jid = try!(Jid::from_str(jid));
        let password = password.to_owned();
        let connect = Self::make_connect(jid.clone(), password.clone(), handle);
        Ok(Client {
            jid,
            state: ClientState::Connecting(connect),
            write_queue: VecDeque::new(),
            iq_tracker: iq::Tracker::new(),
        })
    }

    fn make_connect(jid: Jid, password: String, handle: Handle) -> Box<Future<Item=XMPPStream, Error=String>> {
        let username = jid.node.as_ref().unwrap().to_owned();
        let jid1 = jid.clone();
        let jid2 = jid.clone();
        let password = password;
        let domain = match idna::domain_to_ascii(&jid.domain) {
            Ok(domain) =>
                domain,
            Err(e) =>
                return Box::new(future::err(format!("{:?}", e))),
        };
        Box::new(
            Connecter::from_lookup(handle, &domain, "_xmpp-client._tcp", 5222)
                .expect("Connector::from_lookup")
                .and_then(move |tcp_stream|
                          xmpp_stream::XMPPStream::start(tcp_stream, jid1, NS_JABBER_CLIENT.to_owned())
                          .map_err(|e| format!("{}", e))
                ).and_then(|xmpp_stream| {
                    if Self::can_starttls(&xmpp_stream) {
                        Ok(Self::starttls(xmpp_stream))
                    } else {
                        Err("No STARTTLS".to_owned())
                    }
                }).and_then(|starttls|
                            starttls
                ).and_then(|tls_stream|
                          XMPPStream::start(tls_stream, jid2, NS_JABBER_CLIENT.to_owned())
                          .map_err(|e| format!("{}", e))
                ).and_then(move |xmpp_stream| {
                    Self::auth(xmpp_stream, username, password).expect("auth")
                }).and_then(|xmpp_stream| {
                    Self::bind(xmpp_stream)
                }).and_then(|xmpp_stream| {
                    println!("Bound to {}", xmpp_stream.jid);
                    Ok(xmpp_stream)
                })
        )
    }

    fn can_starttls<S>(stream: &xmpp_stream::XMPPStream<S>) -> bool {
        stream.stream_features
            .get_child("starttls", NS_XMPP_TLS)
            .is_some()
    }

    fn starttls<S: AsyncRead + AsyncWrite>(stream: xmpp_stream::XMPPStream<S>) -> StartTlsClient<S> {
        StartTlsClient::from_stream(stream)
    }

    fn auth<S: AsyncRead + AsyncWrite>(stream: xmpp_stream::XMPPStream<S>, username: String, password: String) -> Result<ClientAuth<S>, String> {
        let creds = Credentials::default()
            .with_username(username)
            .with_password(password)
            .with_channel_binding(ChannelBinding::None);
        ClientAuth::new(stream, creds)
    }

    fn bind<S: AsyncWrite>(stream: xmpp_stream::XMPPStream<S>) -> ClientBind<S> {
        ClientBind::new(stream)
    }

    pub fn write(&mut self, stanza: Element) {
        self.write_queue.push_back(stanza);
    }

    pub fn iq(&mut self, mut stanza: Element) -> iq::IqFuture {
        // generate `id` if missing or duplicate
        while stanza.attr("id").is_none() || self.iq_tracker.has(&stanza) {
            stanza.set_attr("id", iq::generate_id(6));
        }

        let future = self.iq_tracker.insert(&stanza);
        self.write(stanza);
        future
    }
}

impl Stream for Client {
    type Item = Event;
    type Error = String;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let state = replace(&mut self.state, ClientState::Invalid);

        match state {
            ClientState::Invalid =>
                Err("invalid client state".to_owned()),
            ClientState::Disconnected =>
                Ok(Async::Ready(None)),
            ClientState::Connecting(mut connect) => {
                match connect.poll() {
                    Ok(Async::Ready(stream)) => {
                        self.state = ClientState::Connected(stream);
                        Ok(Async::Ready(Some(Event::Online)))
                    },
                    Ok(Async::NotReady) => {
                        self.state = ClientState::Connecting(connect);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(e),
                }
            },
            ClientState::Connected(mut stream) => {
                // Poll sink
                match stream.poll_complete() {
                    Ok(Async::NotReady) => (),
                    Ok(Async::Ready(())) =>
                        match self.write_queue.pop_front() {
                            None => (),
                            Some(element) =>
                                match stream.start_send(Packet::Stanza(element)) {
                                    Ok(AsyncSink::Ready) => (),
                                    Err(e) =>
                                        return Err(e.description().to_owned()),
                                    Ok(AsyncSink::NotReady(Packet::Stanza(element))) =>
                                        self.write_queue.push_front(element),
                                    Ok(AsyncSink::NotReady(_)) =>
                                        unreachable!(),
                                },
                        },
                    Err(e) =>
                        return Err(e.description().to_owned()),
                };

                // Poll stream
                match stream.poll() {
                    Ok(Async::Ready(None)) => {
                        // EOF
                        self.state = ClientState::Disconnected;
                        Ok(Async::Ready(Some(Event::Disconnected)))
                    },
                    Ok(Async::Ready(Some(Packet::Stanza(stanza)))) => {
                        self.state = ClientState::Connected(stream);
                        Ok(Async::Ready(Some(Event::Stanza(stanza))))
                    },
                    Ok(Async::NotReady) |
                    Ok(Async::Ready(_)) => {
                        self.state = ClientState::Connected(stream);
                        Ok(Async::NotReady)
                    },
                    Err(e) =>
                        Err(e.description().to_owned()),
                }
            },
        }
    }
}

impl Sink for Client {
    type SinkItem = Element;
    type SinkError = String;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.state {
            ClientState::Connected(ref mut stream) =>
                match stream.start_send(Packet::Stanza(item)) {
                    Ok(AsyncSink::NotReady(Packet::Stanza(stanza))) =>
                        Ok(AsyncSink::NotReady(stanza)),
                    Ok(AsyncSink::NotReady(_)) =>
                        panic!("Client.start_send with stanza but got something else back"),
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
        match self.state {
            ClientState::Connected(ref mut stream) =>
                stream.poll_complete()
                .map_err(|e| e.description().to_owned()),
            _ =>
                Ok(Async::Ready(())),
        }
    }
}
