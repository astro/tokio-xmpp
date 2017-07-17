use std::mem::replace;
use std::io::{Error, ErrorKind};
use std::collections::HashMap;
use futures::*;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use jid::Jid;

use xmpp_codec::*;
use xmpp_stream::*;

const NS_XMPP_STREAM: &str = "http://etherx.jabber.org/streams";

pub struct StreamStart<S: AsyncWrite> {
    state: StreamStartState<S>,
    jid: Jid,
}

enum StreamStartState<S: AsyncWrite> {
    SendStart(sink::Send<Framed<S, XMPPCodec>>),
    RecvStart(Framed<S, XMPPCodec>),
    RecvFeatures(Framed<S, XMPPCodec>, HashMap<String, String>),
    Invalid,
}

impl<S: AsyncWrite> StreamStart<S> {
    pub fn from_stream(stream: Framed<S, XMPPCodec>, jid: Jid) -> Self {
        let attrs = [("to".to_owned(), jid.domain.clone()),
                     ("version".to_owned(), "1.0".to_owned()),
                     ("xmlns".to_owned(), "jabber:client".to_owned()),
                     ("xmlns:stream".to_owned(), NS_XMPP_STREAM.to_owned()),
        ].iter().cloned().collect();
        let send = stream.send(Packet::StreamStart(attrs));

        StreamStart {
            state: StreamStartState::SendStart(send),
            jid,
        }
    }
}

impl<S: AsyncRead + AsyncWrite> Future for StreamStart<S> {
    type Item = XMPPStream<S>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let old_state = replace(&mut self.state, StreamStartState::Invalid);
        let mut retry = false;

        let (new_state, result) = match old_state {
            StreamStartState::SendStart(mut send) =>
                match send.poll() {
                    Ok(Async::Ready(stream)) => {
                        retry = true;
                        (StreamStartState::RecvStart(stream), Ok(Async::NotReady))
                    },
                    Ok(Async::NotReady) =>
                        (StreamStartState::SendStart(send), Ok(Async::NotReady)),
                    Err(e) =>
                        (StreamStartState::Invalid, Err(e)),
                },
            StreamStartState::RecvStart(mut stream) =>
                match stream.poll() {
                    Ok(Async::Ready(Some(Packet::StreamStart(stream_attrs)))) => {
                        retry = true;
                        // TODO: skip RecvFeatures for version < 1.0
                        (StreamStartState::RecvFeatures(stream, stream_attrs), Ok(Async::NotReady))
                    },
                    Ok(Async::Ready(_)) =>
                        return Err(Error::from(ErrorKind::InvalidData)),
                    Ok(Async::NotReady) =>
                        (StreamStartState::RecvStart(stream), Ok(Async::NotReady)),
                    Err(e) =>
                        return Err(e),
                },
            StreamStartState::RecvFeatures(mut stream, stream_attrs) =>
                match stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(stanza)))) =>
                        if stanza.name() == "features"
                        && stanza.ns() == Some(NS_XMPP_STREAM) {
                            let stream = XMPPStream::new(self.jid.clone(), stream, stream_attrs, stanza);
                            (StreamStartState::Invalid, Ok(Async::Ready(stream)))
                        } else {
                            (StreamStartState::RecvFeatures(stream, stream_attrs), Ok(Async::NotReady))
                        },
                    Ok(Async::Ready(item)) => {
                        println!("StreamStart skip {:?}", item);
                        (StreamStartState::RecvFeatures(stream, stream_attrs), Ok(Async::NotReady))
                    },
                    Ok(Async::NotReady) =>
                        (StreamStartState::RecvFeatures(stream, stream_attrs), Ok(Async::NotReady)),
                    Err(e) =>
                        return Err(e),
                },
            StreamStartState::Invalid =>
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
