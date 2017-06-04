use std::mem::replace;
use std::io::{Error, ErrorKind};
use std::sync::Arc;
use futures::{Future, Sink, Poll, Async};
use futures::stream::Stream;
use futures::sink;
use tokio_core::net::TcpStream;
use rustls::*;
use tokio_rustls::*;
use xml;

use super::{XMPPStream, XMPPCodec, Packet};


const NS_XMPP_STREAM: &str = "http://etherx.jabber.org/streams";
const NS_XMPP_TLS: &str = "urn:ietf:params:xml:ns:xmpp-tls";

pub struct StartTlsClient {
    state: StartTlsClientState,
    arc_config: Arc<ClientConfig>,
}

enum StartTlsClientState {
    Invalid,
    AwaitFeatures(XMPPStream<TcpStream>),
    SendStartTls(sink::Send<XMPPStream<TcpStream>>),
    AwaitProceed(XMPPStream<TcpStream>),
    StartingTls(ConnectAsync<TcpStream>),
}

impl StartTlsClient {
    /// Waits for <stream:features>
    pub fn from_stream(xmpp_stream: XMPPStream<TcpStream>, arc_config: Arc<ClientConfig>) -> Self {
        StartTlsClient {
            state: StartTlsClientState::AwaitFeatures(xmpp_stream),
            arc_config: arc_config,
        }
    }
}

// TODO: eval <stream:features>, check ns
impl Future for StartTlsClient {
    type Item = XMPPStream<TlsStream<TcpStream, ClientSession>>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let old_state = replace(&mut self.state, StartTlsClientState::Invalid);
        let mut retry = false;
        
        let (new_state, result) = match old_state {
            StartTlsClientState::AwaitFeatures(mut xmpp_stream) =>
                match xmpp_stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.name == "features"
                        && stanza.ns == Some(NS_XMPP_STREAM.to_owned())
                        =>
                    {
                        println!("Got features: {}", stanza);
                        match stanza.get_child("starttls", Some(NS_XMPP_TLS)) {
                            None =>
                                (StartTlsClientState::Invalid, Err(Error::from(ErrorKind::InvalidData))),
                            Some(_) => {
                                let nonza = xml::Element::new(
                                    "starttls".to_owned(), Some(NS_XMPP_TLS.to_owned()),
                                    vec![]
                                );
                                println!("send {}", nonza);
                                let packet = Packet::Stanza(nonza);
                                let send = xmpp_stream.send(packet);
                                let new_state = StartTlsClientState::SendStartTls(send);
                                retry = true;
                                (new_state, Ok(Async::NotReady))
                            },
                        }
                    },
                    Ok(Async::Ready(value)) => {
                        println!("StartTlsClient ignore {:?}", value);
                        (StartTlsClientState::AwaitFeatures(xmpp_stream), Ok(Async::NotReady))
                    },
                    Ok(_) =>
                        (StartTlsClientState::AwaitFeatures(xmpp_stream), Ok(Async::NotReady)),
                    Err(e) =>
                        (StartTlsClientState::AwaitFeatures(xmpp_stream), Err(e)),
                },
            StartTlsClientState::SendStartTls(mut send) =>
                match send.poll() {
                    Ok(Async::Ready(xmpp_stream)) => {
                        println!("starttls sent");
                        let new_state = StartTlsClientState::AwaitProceed(xmpp_stream);
                        retry = true;
                        (new_state, Ok(Async::NotReady))
                    },
                    Ok(Async::NotReady) =>
                        (StartTlsClientState::SendStartTls(send), Ok(Async::NotReady)),
                    Err(e) =>
                        (StartTlsClientState::SendStartTls(send), Err(e)),
                },
            StartTlsClientState::AwaitProceed(mut xmpp_stream) =>
                match xmpp_stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.name == "proceed" =>
                    {
                        println!("* proceed *");
                        let stream = xmpp_stream.into_inner();
                        let connect = self.arc_config.connect_async("spaceboyz.net", stream);
                        let new_state = StartTlsClientState::StartingTls(connect);
                        retry = true;
                        (new_state, Ok(Async::NotReady))
                    },
                    Ok(Async::Ready(value)) => {
                        println!("StartTlsClient ignore {:?}", value);
                        (StartTlsClientState::AwaitFeatures(xmpp_stream), Ok(Async::NotReady))
                    },
                    Ok(_) =>
                        (StartTlsClientState::AwaitProceed(xmpp_stream), Ok(Async::NotReady)),
                    Err(e) =>
                        (StartTlsClientState::AwaitProceed(xmpp_stream),  Err(e)),
                },
            StartTlsClientState::StartingTls(mut connect) =>
                match connect.poll() {
                    Ok(Async::Ready(tls_stream)) => {
                        println!("Got a TLS stream!");
                        let xmpp_stream = XMPPCodec::frame_stream(tls_stream);
                        (StartTlsClientState::Invalid, Ok(Async::Ready(xmpp_stream)))
                    },
                    Ok(Async::NotReady) =>
                        (StartTlsClientState::StartingTls(connect), Ok(Async::NotReady)),
                    Err(e) =>
                        (StartTlsClientState::StartingTls(connect),  Err(e)),
                },
            StartTlsClientState::Invalid =>
                unreachable!(),
        };

        self.state = new_state;
        if retry {
            self.poll()
        } else {
            result
        }
    }
}
