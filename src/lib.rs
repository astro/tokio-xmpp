#[macro_use]
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate bytes;
extern crate xml;
extern crate rustls;
extern crate tokio_rustls;


pub mod xmpp_codec;
pub mod xmpp_stream;
mod stream_start;
mod tcp;
pub use tcp::*;
mod starttls;
pub use starttls::*;


// type FullClient = sasl::Client<StartTLS<TCPConnection>>

