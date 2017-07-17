use std::mem::replace;
use std::str::FromStr;
use std::error::Error;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tls::TlsStream;
use futures::*;
use minidom::Element;
use jid::{Jid, JidParseError};
use sasl::common::{Credentials, ChannelBinding};

use super::xmpp_codec::Packet;
use super::xmpp_stream;
use super::tcp::TcpClient;
use super::starttls::{NS_XMPP_TLS, StartTlsClient};
use super::happy_eyeballs::Connecter;

mod auth;
use self::auth::*;
mod bind;
use self::bind::*;
mod event;
pub use self::event::Event as ClientEvent;

pub struct Client {
    pub jid: Jid,
    state: ClientState,
}

type XMPPStream = xmpp_stream::XMPPStream<TlsStream<TcpStream>>;

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
        })
    }

    fn make_connect(jid: Jid, password: String, handle: Handle) -> Box<Future<Item=XMPPStream, Error=String>> {
        let username = jid.node.as_ref().unwrap().to_owned();
        let password = password;
        Box::new(
            Connecter::from_lookup(handle, &jid.domain, "_xmpp-client._tcp", 5222)
                .expect("Connector::from_lookup")
                .and_then(|tcp_stream|
                          TcpClient::from_stream(jid, tcp_stream)
                          .map_err(|e| format!("{}", e))
                ).and_then(|stream| {
                    if Self::can_starttls(&stream) {
                        Self::starttls(stream)
                    } else {
                        panic!("No STARTTLS")
                    }
                }).and_then(move |stream| {
                    Self::auth(stream, username, password).expect("auth")
                }).and_then(|stream| {
                    Self::bind(stream)
                }).and_then(|stream| {
                    println!("Bound to {}", stream.jid);
                    Ok(stream)
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
}

impl Stream for Client {
    type Item = ClientEvent;
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
                        Ok(Async::Ready(Some(ClientEvent::Online)))
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
                match stream.poll() {
                    Ok(Async::NotReady) => {
                        self.state = ClientState::Connected(stream);
                        Ok(Async::NotReady)
                    },
                    Ok(Async::Ready(None)) => {
                        // EOF
                        self.state = ClientState::Disconnected;
                        Ok(Async::Ready(Some(ClientEvent::Disconnected)))
                    },
                    Ok(Async::Ready(Some(Packet::Stanza(stanza)))) => {
                        self.state = ClientState::Connected(stream);
                        Ok(Async::Ready(Some(ClientEvent::Stanza(stanza))))
                    },
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
        match &mut self.state {
            &mut ClientState::Connected(ref mut stream) =>
                stream.poll_complete()
                .map_err(|e| e.description().to_owned()),
            _ =>
                Ok(Async::Ready(())),
        }
    }
}
