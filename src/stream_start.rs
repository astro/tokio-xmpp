use std::mem::replace;
// use std::error::Error as StdError;
// use std::{fmt, io};
use futures::{Future, Async, Poll, Stream, sink, Sink};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_codec::Framed;
use jid::Jid;
use minidom::Element;

use xmpp_codec::{XMPPCodec, Packet};
use xmpp_stream::XMPPStream;
use {Error, ProtocolError};

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
                        let stream_ns = match stream_attrs.get("xmlns") {
                            Some(ns) => ns.clone(),
                            None =>
                                return Err(ProtocolError::NoStreamNamespace.into()),
                        };
                        if self.ns == "jabber:client" {
                            retry = true;
                            // TODO: skip RecvFeatures for version < 1.0
                            (StreamStartState::RecvFeatures(stream, stream_ns), Ok(Async::NotReady))
                        } else {
                            let id = match stream_attrs.get("id") {
                                Some(id) => id.clone(),
                                None =>
                                    return Err(ProtocolError::NoStreamId.into()),
                            };
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

// #[derive(Debug)]
// pub enum StreamStartError {
//     MissingStreamNs,
//     MissingStreamId,
//     Unexpected,
//     Parser(ParserError),
//     IO(io::Error),
// }

// impl From<io::Error> for StreamStartError {
//     fn from(e: io::Error) -> Self {
//         StreamStartError::IO(e)
//     }
// }

// impl From<ParserError> for StreamStartError {
//     fn from(e: ParserError) -> Self {
//         match e {
//             ParserError::IO(e) => StreamStartError::IO(e),
//             _ => StreamStartError::Parser(e)
//         }
//     }
// }

// impl StdError for StreamStartError {
//     fn description(&self) -> &str {
//         match *self {
//             StreamStartError::MissingStreamNs => "Missing stream namespace",
//             StreamStartError::MissingStreamId => "Missing stream id",
//             StreamStartError::Unexpected => "Unexpected",
//             StreamStartError::Parser(ref pe) => pe.description(),
//             StreamStartError::IO(ref ie) => ie.description(),
//         }
//     }

//     fn cause(&self) -> Option<&StdError> {
//         match *self {
//             StreamStartError::Parser(ref pe) => pe.cause(),
//             StreamStartError::IO(ref ie) => ie.cause(),
//             _ => None,
//         }
//     }
// }

// impl fmt::Display for StreamStartError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match *self {
//             StreamStartError::MissingStreamNs => write!(f, "Missing stream namespace"),
//             StreamStartError::MissingStreamId => write!(f, "Missing stream id"),
//             StreamStartError::Unexpected => write!(f, "Received unexpected data"),
//             StreamStartError::Parser(ref pe) => write!(f, "{}", pe),
//             StreamStartError::IO(ref ie) => write!(f, "{}", ie),
//         }
//     }
// }
