use futures::{Poll, Stream, Sink, StartSend};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use minidom::Element;
use jid::Jid;

use xmpp_codec::XMPPCodec;
use stream_start::StreamStart;

pub const NS_XMPP_STREAM: &str = "http://etherx.jabber.org/streams";

pub struct XMPPStream<S> {
    pub jid: Jid,
    pub stream: Framed<S, XMPPCodec>,
    pub stream_features: Element,
    pub ns: String,
}

impl<S: AsyncRead + AsyncWrite> XMPPStream<S> {
    pub fn new(jid: Jid,
               stream: Framed<S, XMPPCodec>,
               ns: String,
               stream_features: Element) -> Self {
        XMPPStream { jid, stream, stream_features, ns }
    }

    pub fn start(stream: S, jid: Jid, ns: String) -> StreamStart<S> {
        let xmpp_stream = AsyncRead::framed(stream, XMPPCodec::new());
        StreamStart::from_stream(xmpp_stream, jid, ns)
    }

    pub fn into_inner(self) -> S {
        self.stream.into_inner()
    }

    pub fn restart(self) -> StreamStart<S> {
        Self::start(self.stream.into_inner(), self.jid, self.ns)
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
