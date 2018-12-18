#![deny(unsafe_code, unused, missing_docs)]

//! XMPP implemeentation with asynchronous I/O using Tokio.

#[macro_use]
extern crate derive_error;

mod starttls;
mod stream_start;
pub mod xmpp_codec;
pub mod xmpp_stream;
pub use crate::starttls::StartTlsClient;
mod event;
mod happy_eyeballs;
pub use crate::event::Event;
mod client;
pub use crate::client::Client;
mod component;
pub use crate::component::Component;
mod error;
pub use crate::error::{AuthError, ConnecterError, Error, ParseError, ParserError, ProtocolError};
