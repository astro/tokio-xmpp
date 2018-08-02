//! `XMPPStream` is the common container for all XMPP network connections

use futures::{Poll, Stream, Sink, StartSend};
use futures::sink::Send;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_codec::Framed;
use minidom::Element;
use jid::Jid;

use xmpp_codec::{XMPPCodec, Packet};
use stream_start::StreamStart;

/// <stream:stream> namespace
pub const NS_XMPP_STREAM: &str = "http://etherx.jabber.org/streams";

/// Wraps a `stream`
pub struct XMPPStream<S> {
    /// The local Jabber-Id
    pub jid: Jid,
    /// Codec instance
    pub stream: Framed<S, XMPPCodec>,
    /// `<stream:features/>` for XMPP version 1.0
    pub stream_features: Element,
    /// Root namespace
    ///
    /// This is different for either c2s, s2s, or component
    /// connections.
    pub ns: String,
}

impl<S: AsyncRead + AsyncWrite> XMPPStream<S> {
    /// Constructor
    pub fn new(jid: Jid,
               stream: Framed<S, XMPPCodec>,
               ns: String,
               stream_features: Element) -> Self {
        XMPPStream { jid, stream, stream_features, ns }
    }

    /// Send a `<stream:stream>` start tag
    pub fn start(stream: S, jid: Jid, ns: String) -> StreamStart<S> {
        let xmpp_stream = Framed::new(stream, XMPPCodec::new());
        StreamStart::from_stream(xmpp_stream, jid, ns)
    }

    /// Unwraps the inner stream
    pub fn into_inner(self) -> S {
        self.stream.into_inner()
    }

    /// Re-run `start()`
    pub fn restart(self) -> StreamStart<S> {
        Self::start(self.stream.into_inner(), self.jid, self.ns)
    }
}

impl<S: AsyncWrite> XMPPStream<S> {
    /// Convenience method
    pub fn send_stanza<E: Into<Element>>(self, e: E) -> Send<Self> {
        self.send(Packet::Stanza(e.into()))
    }
}

/// Proxy to self.stream
impl<S: AsyncWrite> Sink for XMPPStream<S> {
    type SinkItem = <Framed<S, XMPPCodec> as Sink>::SinkItem;
    type SinkError = <Framed<S, XMPPCodec> as Sink>::SinkError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.stream.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.stream.poll_complete()
    }
}

/// Proxy to self.stream
impl<S: AsyncRead> Stream for XMPPStream<S> {
    type Item = <Framed<S, XMPPCodec> as Stream>::Item;
    type Error = <Framed<S, XMPPCodec> as Stream>::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.stream.poll()
    }
}
