use std::mem::replace;
use std::str::FromStr;
use std::error::Error;
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tls::TlsStream;
use futures::*;
use jid::{Jid, JidParseError};
use xml;
use sasl::common::{Credentials, ChannelBinding};

use super::xmpp_codec::Packet;
use super::xmpp_stream;
use super::tcp::TcpClient;
use super::starttls::{NS_XMPP_TLS, StartTlsClient};

mod auth;
use self::auth::*;
mod bind;
use self::bind::*;

pub struct Client {
    pub jid: Jid,
    password: String,
    state: ClientState,
}

type XMPPStream = xmpp_stream::XMPPStream<TlsStream<TcpStream>>;

enum ClientState {
    Invalid,
    Disconnected,
    Connecting(Box<Future<Item=XMPPStream, Error=String>>),
    Connected(XMPPStream),
    // Sending,
    // Drain,
}

impl Client {
    pub fn new(jid: &str, password: &str, handle: &Handle) -> Result<Self, JidParseError> {
        let jid = try!(Jid::from_str(jid));
        let password = password.to_owned();
        let connect = Self::make_connect(jid.clone(), password.clone(), handle);
        Ok(Client {
            jid, password,
            state: ClientState::Connecting(connect),
        })
    }

    fn make_connect(jid: Jid, password: String, handle: &Handle) -> Box<Future<Item=XMPPStream, Error=String>> {
        use std::net::ToSocketAddrs;
        let addr = "89.238.79.220:5222"
            .to_socket_addrs().unwrap()
            .next().unwrap();
        let username = jid.node.as_ref().unwrap().to_owned();
        let password = password;
        Box::new(
            TcpClient::connect(
                jid,
                &addr,
                handle
            ).map_err(|e| format!("{}", e)
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

                let presence = xml::Element::new("presence".to_owned(), None, vec![]);
                stream.send(Packet::Stanza(presence))
                    .map_err(|e| format!("{}", e))
            })
        )
    }

    fn can_starttls<S>(stream: &xmpp_stream::XMPPStream<S>) -> bool {
        stream.stream_features
            .get_child("starttls", Some(NS_XMPP_TLS))
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

#[derive(Debug)]
pub enum ClientEvent {
    Online,
    Disconnected,
    Stanza(xml::Element),
}

impl Stream for Client {
    type Item = ClientEvent;
    type Error = String;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        println!("stream.poll");
        let state = replace(&mut self.state, ClientState::Invalid);

        match state {
            ClientState::Invalid =>
                Err("invalid client state".to_owned()),
            ClientState::Disconnected =>
                Ok(Async::NotReady),
            ClientState::Connecting(mut connect) => {
                match connect.poll() {
                    Ok(Async::Ready(stream)) => {
                        println!("connected");
                        self.state = ClientState::Connected(stream);
                        self.poll()
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
