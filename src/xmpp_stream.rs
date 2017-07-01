use std::collections::HashMap;
use futures::*;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use xml;
use jid::Jid;

use xmpp_codec::*;
use stream_start::*;

pub const NS_XMPP_STREAM: &str = "http://etherx.jabber.org/streams";

pub struct XMPPStream<S> {
    pub jid: Jid,
    pub stream: Framed<S, XMPPCodec>,
    pub stream_attrs: HashMap<String, String>,
    pub stream_features: xml::Element,
}

impl<S: AsyncRead + AsyncWrite> XMPPStream<S> {
    pub fn new(jid: Jid,
               stream: Framed<S, XMPPCodec>,
               stream_attrs: HashMap<String, String>,
               stream_features: xml::Element) -> Self {
        XMPPStream { jid, stream, stream_attrs, stream_features }
    }

    pub fn from_stream(stream: S, jid: Jid) -> StreamStart<S> {
        let xmpp_stream = AsyncRead::framed(stream, XMPPCodec::new());
        StreamStart::from_stream(xmpp_stream, jid)
    }

    pub fn into_inner(self) -> S {
        self.stream.into_inner()
    }

    pub fn restart(self) -> StreamStart<S> {
        Self::from_stream(self.stream.into_inner(), self.jid)
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
