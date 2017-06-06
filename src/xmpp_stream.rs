use std::default::Default;
use std::collections::HashMap;
use futures::*;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use xml;
use sasl::common::Credentials;

use xmpp_codec::*;
use stream_start::*;
use starttls::{NS_XMPP_TLS, StartTlsClient};
use client_auth::ClientAuth;

pub const NS_XMPP_STREAM: &str = "http://etherx.jabber.org/streams";

pub struct XMPPStream<S> {
    pub stream: Framed<S, XMPPCodec>,
    pub stream_attrs: HashMap<String, String>,
    pub stream_features: xml::Element,
}

impl<S: AsyncRead + AsyncWrite> XMPPStream<S> {
    pub fn from_stream(stream: S, to: String) -> StreamStart<S> {
        let xmpp_stream = AsyncRead::framed(stream, XMPPCodec::new());
        StreamStart::from_stream(xmpp_stream, to)
    }

    pub fn into_inner(self) -> S {
        self.stream.into_inner()
    }

    pub fn restart(self) -> StreamStart<S> {
        let to = self.stream_attrs.get("from")
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "".to_owned());
        Self::from_stream(self.into_inner(), to.clone())
    }

    pub fn can_starttls(&self) -> bool {
        self.stream_features
            .get_child("starttls", Some(NS_XMPP_TLS))
            .is_some()
    }

    pub fn starttls(self) -> StartTlsClient<S> {
        StartTlsClient::from_stream(self)
    }

    pub fn auth(self, username: &str, password: &str) -> Result<ClientAuth<S>, String> {
        let creds = Credentials::default()
            .with_username(username)
            .with_password(password);
        ClientAuth::new(self, creds)
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
