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


pub mod xmpp_codec;
pub mod xmpp_stream;
mod stream_start;
mod tcp;
pub use tcp::*;
mod starttls;
pub use starttls::*;
mod client_auth;
pub use client_auth::*;
mod client_bind;
pub use client_bind::*;


// type FullClient = sasl::Client<StartTLS<TCPConnection>>

