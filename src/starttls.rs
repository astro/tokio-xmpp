use std::mem::replace;
use futures::{Future, Sink, Poll, Async};
use futures::stream::Stream;
use futures::sink;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tls::*;
use native_tls::TlsConnector;
use xml;
use jid::Jid;

use xmpp_codec::*;
use xmpp_stream::*;
use stream_start::StreamStart;


pub const NS_XMPP_TLS: &str = "urn:ietf:params:xml:ns:xmpp-tls";

pub struct StartTlsClient<S: AsyncRead + AsyncWrite> {
    state: StartTlsClientState<S>,
    jid: Jid,
}

enum StartTlsClientState<S: AsyncRead + AsyncWrite> {
    Invalid,
    SendStartTls(sink::Send<XMPPStream<S>>),
    AwaitProceed(XMPPStream<S>),
    StartingTls(ConnectAsync<S>),
    Start(StreamStart<TlsStream<S>>),
}

impl<S: AsyncRead + AsyncWrite> StartTlsClient<S> {
    /// Waits for <stream:features>
    pub fn from_stream(xmpp_stream: XMPPStream<S>) -> Self {
        let jid = xmpp_stream.jid.clone();

        let nonza = xml::Element::new(
            "starttls".to_owned(), Some(NS_XMPP_TLS.to_owned()),
            vec![]
        );
        println!("send {}", nonza);
        let packet = Packet::Stanza(nonza);
        let send = xmpp_stream.send(packet);

        StartTlsClient {
            state: StartTlsClientState::SendStartTls(send),
            jid,
        }
    }
}

impl<S: AsyncRead + AsyncWrite> Future for StartTlsClient<S> {
    type Item = XMPPStream<TlsStream<S>>;
    type Error = String;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let old_state = replace(&mut self.state, StartTlsClientState::Invalid);
        let mut retry = false;
        
        let (new_state, result) = match old_state {
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
                        (StartTlsClientState::SendStartTls(send), Err(format!("{}", e))),
                },
            StartTlsClientState::AwaitProceed(mut xmpp_stream) =>
                match xmpp_stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(ref stanza))))
                        if stanza.name == "proceed" =>
                    {
                        println!("* proceed *");
                        let stream = xmpp_stream.stream.into_inner();
                        let connect = TlsConnector::builder().unwrap()
                            .build().unwrap()
                            .connect_async(&self.jid.domain, stream);
                        let new_state = StartTlsClientState::StartingTls(connect);
                        retry = true;
                        (new_state, Ok(Async::NotReady))
                    },
                    Ok(Async::Ready(value)) => {
                        println!("StartTlsClient ignore {:?}", value);
                        (StartTlsClientState::AwaitProceed(xmpp_stream), Ok(Async::NotReady))
                    },
                    Ok(_) =>
                        (StartTlsClientState::AwaitProceed(xmpp_stream), Ok(Async::NotReady)),
                    Err(e) =>
                        (StartTlsClientState::AwaitProceed(xmpp_stream),  Err(format!("{}", e))),
                },
            StartTlsClientState::StartingTls(mut connect) =>
                match connect.poll() {
                    Ok(Async::Ready(tls_stream)) => {
                        println!("Got a TLS stream!");
                        let start = XMPPStream::from_stream(tls_stream, self.jid.clone());
                        let new_state = StartTlsClientState::Start(start);
                        retry = true;
                        (new_state, Ok(Async::NotReady))
                    },
                    Ok(Async::NotReady) =>
                        (StartTlsClientState::StartingTls(connect), Ok(Async::NotReady)),
                    Err(e) =>
                        (StartTlsClientState::StartingTls(connect),  Err(format!("{}", e))),
                },
            StartTlsClientState::Start(mut start) =>
                match start.poll() {
                    Ok(Async::Ready(xmpp_stream)) =>
                        (StartTlsClientState::Invalid, Ok(Async::Ready(xmpp_stream))),
                    Ok(Async::NotReady) =>
                        (StartTlsClientState::Start(start), Ok(Async::NotReady)),
                    Err(e) =>
                        (StartTlsClientState::Invalid, Err(format!("{}", e))),
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
