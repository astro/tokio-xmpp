use std::mem::replace;
use futures::{Future, Async, Poll, Stream, sink, Sink};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_codec::Framed;
use jid::Jid;
use minidom::Element;

use crate::xmpp_codec::{XMPPCodec, Packet};
use crate::xmpp_stream::XMPPStream;
use crate::{Error, ProtocolError};

const NS_XMPP_STREAM: &str = "http://etherx.jabber.org/streams";

pub struct StreamStart<S: AsyncWrite> {
    state: StreamStartState<S>,
    jid: Jid,
    ns: String,
}

enum StreamStartState<S: AsyncWrite> {
    SendStart(sink::Send<Framed<S, XMPPCodec>>),
    RecvStart(Framed<S, XMPPCodec>),
    RecvFeatures(Framed<S, XMPPCodec>, String),
    Invalid,
}

impl<S: AsyncWrite> StreamStart<S> {
    pub fn from_stream(stream: Framed<S, XMPPCodec>, jid: Jid, ns: String) -> Self {
        let attrs = [("to".to_owned(), jid.domain.clone()),
                     ("version".to_owned(), "1.0".to_owned()),
                     ("xmlns".to_owned(), ns.clone()),
                     ("xmlns:stream".to_owned(), NS_XMPP_STREAM.to_owned()),
        ].iter().cloned().collect();
        let send = stream.send(Packet::StreamStart(attrs));

        StreamStart {
            state: StreamStartState::SendStart(send),
            jid,
            ns,
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
                        (StreamStartState::Invalid, Err(e.into())),
                },
            StreamStartState::RecvStart(mut stream) =>
                match stream.poll() {
                    Ok(Async::Ready(Some(Packet::StreamStart(stream_attrs)))) => {
                        let stream_ns = stream_attrs.get("xmlns")
                            .ok_or(ProtocolError::NoStreamNamespace)?
                            .clone();
                        if self.ns == "jabber:client" {
                            retry = true;
                            // TODO: skip RecvFeatures for version < 1.0
                            (StreamStartState::RecvFeatures(stream, stream_ns), Ok(Async::NotReady))
                        } else {
                            let id = stream_attrs.get("id")
                                .ok_or(ProtocolError::NoStreamId)?
                                .clone();
                            // FIXME: huge hack, shouldn’t be an element!
                            let stream = XMPPStream::new(self.jid.clone(), stream, self.ns.clone(), Element::builder(id).build());
                            (StreamStartState::Invalid, Ok(Async::Ready(stream)))
                        }
                    },
                    Ok(Async::Ready(_)) =>
                        return Err(ProtocolError::InvalidToken.into()),
                    Ok(Async::NotReady) =>
                        (StreamStartState::RecvStart(stream), Ok(Async::NotReady)),
                    Err(e) =>
                        return Err(ProtocolError::from(e).into()),
                },
            StreamStartState::RecvFeatures(mut stream, stream_ns) =>
                match stream.poll() {
                    Ok(Async::Ready(Some(Packet::Stanza(stanza)))) =>
                        if stanza.is("features", NS_XMPP_STREAM) {
                            let stream = XMPPStream::new(self.jid.clone(), stream, self.ns.clone(), stanza);
                            (StreamStartState::Invalid, Ok(Async::Ready(stream)))
                        } else {
                            (StreamStartState::RecvFeatures(stream, stream_ns), Ok(Async::NotReady))
                        },
                    Ok(Async::Ready(_)) | Ok(Async::NotReady) =>
                        (StreamStartState::RecvFeatures(stream, stream_ns), Ok(Async::NotReady)),
                    Err(e) =>
                        return Err(ProtocolError::from(e).into()),
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
