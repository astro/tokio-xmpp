#[macro_use]
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate bytes;
extern crate xml;


mod xmpp_codec;
pub use xmpp_codec::*;
mod tcp;
pub use tcp::*;


// type FullClient = sasl::Client<StartTLS<TCPConnection>>

