#![deny(unsafe_code, unused, missing_docs)]

//! XMPP implemeentation with asynchronous I/O using Tokio.

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
