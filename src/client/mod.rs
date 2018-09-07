use std::mem::replace;
use std::str::FromStr;
use tokio::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tls::TlsStream;
use futures::{Future, Stream, Poll, Async, Sink, StartSend, AsyncSink, done};
use minidom::Element;
use jid::{Jid, JidParseError};
use sasl::common::{Credentials, ChannelBinding};
use idna;

use super::xmpp_codec::Packet;
use super::xmpp_stream;
use super::starttls::{NS_XMPP_TLS, StartTlsClient};
use super::happy_eyeballs::Connecter;
use super::event::Event;
use super::{Error, ProtocolError};

mod auth;
use self::auth::ClientAuth;
mod bind;
use self::bind::ClientBind;

/// XMPP client connection and state
pub struct Client {
    /// The client's current Jabber-Id
    pub jid: Jid,
    state: ClientState,
}

type XMPPStream = xmpp_stream::XMPPStream<TlsStream<TcpStream>>;
const NS_JABBER_CLIENT: &str = "jabber:client";

enum ClientState {
    Invalid,
    Disconnected,
    Connecting(Box<Future<Item=XMPPStream, Error=Error>>),
    Connected(XMPPStream),
}

impl Client {
    /// Start a new XMPP client
    ///
    /// Start polling the returned instance so that it will connect
    /// and yield events.
    pub fn new(jid: &str, password: &str) -> Result<Self, JidParseError> {
        let jid = Jid::from_str(jid)?;
        let password = password.to_owned();
        let connect = Self::make_connect(jid.clone(), password.clone());
        Ok(Client {
            jid,
            state: ClientState::Connecting(Box::new(connect)),
        })
    }

    fn make_connect(jid: Jid, password: String) -> impl Future<Item=XMPPStream, Error=Error> {
        let username = jid.node.as_ref().unwrap().to_owned();
        let jid1 = jid.clone();
        let jid2 = jid.clone();
        let password = password;
        done(idna::domain_to_ascii(&jid.domain))
            .map_err(|_| Error::Idna)
            .and_then(|domain|
                      done(Connecter::from_lookup(&domain, Some("_xmpp-client._tcp"), 5222))
                      .map_err(Error::Connection)
            )
            .flatten()
            .and_then(move |tcp_stream|
                      xmpp_stream::XMPPStream::start(tcp_stream, jid1, NS_JABBER_CLIENT.to_owned())
            ).and_then(|xmpp_stream| {
                if Self::can_starttls(&xmpp_stream) {
                    Ok(Self::starttls(xmpp_stream))
                } else {
                    Err(Error::Protocol(ProtocolError::NoTls))
                }
            }).flatten()
            .and_then(|tls_stream|
                       XMPPStream::start(tls_stream, jid2, NS_JABBER_CLIENT.to_owned())
            ).and_then(move |xmpp_stream|
                       done(Self::auth(xmpp_stream, username, password))
                       // TODO: flatten?
            ).and_then(|auth| auth)
            .and_then(|xmpp_stream| {
                Self::bind(xmpp_stream)
            }).and_then(|xmpp_stream| {
                // println!("Bound to {}", xmpp_stream.jid);
                Ok(xmpp_stream)
            })
    }

    fn can_starttls<S>(stream: &xmpp_stream::XMPPStream<S>) -> bool {
        stream.stream_features
            .get_child("starttls", NS_XMPP_TLS)
            .is_some()
    }

    fn starttls<S: AsyncRead + AsyncWrite>(stream: xmpp_stream::XMPPStream<S>) -> StartTlsClient<S> {
        StartTlsClient::from_stream(stream)
    }

    fn auth<S: AsyncRead + AsyncWrite>(stream: xmpp_stream::XMPPStream<S>, username: String, password: String) -> Result<ClientAuth<S>, Error> {
        let creds = Credentials::default()
            .with_username(username)
            .with_password(password)
            .with_channel_binding(ChannelBinding::None);
        ClientAuth::new(stream, creds)
    }

    fn bind<S: AsyncWrite>(stream: xmpp_stream::XMPPStream<S>) -> ClientBind<S> {
        ClientBind::new(stream)
    }
}

impl Stream for Client {
    type Item = Event;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let state = replace(&mut self.state, ClientState::Invalid);

        match state {
            ClientState::Invalid =>
                Err(Error::InvalidState),
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
                    Ok(Async::Ready(())) => (),
                    Err(e) =>
                        return Err(e)?,
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
                        Err(e)?,
                }
            },
        }
    }
}

impl Sink for Client {
    type SinkItem = Element;
    type SinkError = Error;

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
                        Err(e)?,
                },
            _ =>
                Ok(AsyncSink::NotReady(item)),
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        match self.state {
            ClientState::Connected(ref mut stream) =>
                stream.poll_complete()
                .map_err(|e| e.into()),
            _ =>
                Ok(Async::Ready(())),
        }
    }
}
