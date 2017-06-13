use std::default::Default;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use futures::*;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use xml;
use sasl::common::Credentials;

use iq::*;
use xmpp_codec::*;
use stream_start::*;
use starttls::{NS_XMPP_TLS, StartTlsClient};
use client_auth::ClientAuth;

pub const NS_XMPP_STREAM: &str = "http://etherx.jabber.org/streams";

pub struct XMPPStream<S: AsyncRead + AsyncWrite> {
    pub stream: Framed<S, XMPPCodec>,
    pub stream_attrs: HashMap<String, String>,
    pub stream_features: xml::Element,
    pending_iq: HashMap<String, Iq<S>>,
}

impl<S: AsyncRead + AsyncWrite> XMPPStream<S> {
    pub fn new(
        stream: Framed<S, XMPPCodec>,
        stream_attrs: HashMap<String, String>,
        stream_features: xml::Element
    ) -> Self {
        XMPPStream {
            stream,
            stream_attrs,
            stream_features,
            pending_iq: HashMap::new(),
        }
    }
    
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

    /// Once authed, access to the stream can be shared. See SharedXMPPStream<S>
    pub fn share(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }
}

/// Proxy to self.stream
impl<S: AsyncRead + AsyncWrite> Sink for XMPPStream<S> {
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
impl<S: AsyncRead + AsyncWrite> Stream for XMPPStream<S> {
    type Item = <Framed<S, XMPPCodec> as Stream>::Item;
    type Error = <Framed<S, XMPPCodec> as Stream>::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.stream.poll()
    }
}

pub trait SharedXMPPStream<S: AsyncRead + AsyncWrite> {
    fn iq(&self, stanza: xml::Element) -> Iq<S>;
}

impl<S: AsyncRead + AsyncWrite> SharedXMPPStream<S> for Arc<Mutex<XMPPStream<S>>> {
    fn iq(&self, mut stanza: xml::Element) -> Iq<S> {
        let mut id = None;

        let mut stream = self.lock().unwrap();
        while id.is_none() {
            let new_id = generate_id(8);
            if stream.pending_iq.get(&new_id).is_none() {
                id = Some(new_id);
            }
        }
        let id = id.unwrap();
        stanza.attributes.insert(("id".to_owned(), None), id.clone());

        let iq = Iq::send(self.clone(), stanza);
        stream.pending_iq.insert(id, iq.clone());
        iq
    }
}
