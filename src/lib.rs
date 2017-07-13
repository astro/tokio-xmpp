#[macro_use]
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate bytes;
extern crate xml;
extern crate native_tls;
extern crate tokio_tls;
extern crate sasl;
extern crate rustc_serialize as serialize;
extern crate jid;
extern crate domain;

pub mod xmpp_codec;
pub mod xmpp_stream;
mod stream_start;
mod tcp;
pub use tcp::*;
mod starttls;
pub use starttls::*;
mod happy_eyeballs;
mod client;
pub use client::{Client, ClientEvent};
