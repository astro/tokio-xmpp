#![deny(unsafe_code, unused, missing_docs)]

//! XMPP implemeentation with asynchronous I/O using Tokio.

extern crate futures;
extern crate tokio;
extern crate tokio_io;
extern crate tokio_codec;
extern crate bytes;
extern crate xml5ever;
extern crate quick_xml;
extern crate minidom;
extern crate native_tls;
extern crate tokio_tls;
extern crate sasl;
extern crate jid;
extern crate trust_dns_resolver;
extern crate trust_dns_proto;
extern crate idna;
extern crate xmpp_parsers;
extern crate try_from;
#[macro_use]
extern crate derive_error;

pub mod xmpp_codec;
pub mod xmpp_stream;
mod stream_start;
mod starttls;
pub use crate::starttls::StartTlsClient;
mod happy_eyeballs;
mod event;
pub use crate::event::Event;
mod client;
pub use crate::client::Client;
mod component;
pub use crate::component::Component;
mod error;
pub use crate::error::{Error, ProtocolError, AuthError, ConnecterError, ParseError, ParserError};
