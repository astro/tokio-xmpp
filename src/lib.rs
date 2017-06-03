#[macro_use]
extern crate futures;
extern crate tokio_core;
extern crate xml;
extern crate rustls;
extern crate tokio_rustls;


mod xmpp_codec;
pub use xmpp_codec::*;
mod tcp;
pub use tcp::*;


// type FullClient = sasl::Client<StartTLS<TCPConnection>>

