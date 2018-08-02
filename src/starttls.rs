use std::mem::replace;
use futures::{Future, Sink, Poll, Async};
use futures::stream::Stream;
use futures::sink;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tls::{TlsStream, TlsConnectorExt, ConnectAsync};
use native_tls::TlsConnector;
use minidom::Element;
use jid::Jid;

use xmpp_codec::Packet;
use xmpp_stream::XMPPStream;

/// XMPP TLS XML namespace
pub const NS_XMPP_TLS: &str = "urn:ietf:params:xml:ns:xmpp-tls";


/// XMPP stream that switches to TLS if available in received features
pub struct StartTlsClient<S: AsyncRead + AsyncWrite> {
    state: StartTlsClientState<S>,
    jid: Jid,
}

enum StartTlsClientState<S: AsyncRead + AsyncWrite> {
    Invalid,
    SendStartTls(sink::Send<XMPPStream<S>>),
    AwaitProceed(XMPPStream<S>),
    StartingTls(ConnectAsync<S>),
}

impl<S: AsyncRead + AsyncWrite> StartTlsClient<S> {
    /// Waits for <stream:features>
    pub fn from_stream(xmpp_stream: XMPPStream<S>) -> Self {
        let jid = xmpp_stream.jid.clone();

        let nonza = Element::builder("starttls")
            .ns(NS_XMPP_TLS)
            .build();
        let packet = Packet::Stanza(nonza);
        let send = xmpp_stream.send(packet);

        StartTlsClient {
            state: StartTlsClientState::SendStartTls(send),
            jid,
        }
    }
}

impl<S: AsyncRead + AsyncWrite> Future for StartTlsClient<S> {
    type Item = TlsStream<S>;
    type Error = String;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let old_state = replace(&mut self.state, StartTlsClientState::Invalid);
        let mut retry = false;
        
        let (new_state, result) = match old_state {
            StartTlsClientState::SendStartTls(mut send) =>
                match send.poll() {
                    Ok(Async::Ready(xmpp_stream)) => {
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
                        if stanza.name() == "proceed" =>
                    {
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
                    Ok(Async::Ready(tls_stream)) =>
                        (StartTlsClientState::Invalid, Ok(Async::Ready(tls_stream))),
                    Ok(Async::NotReady) =>
                        (StartTlsClientState::StartingTls(connect), Ok(Async::NotReady)),
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
