#[macro_use]
extern crate futures;
extern crate tokio_core;
extern crate xml;

use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::io::ErrorKind;
use futures::{Future, BoxFuture, Sink, Poll, Async};
use futures::stream::{Stream, iter};
use futures::future::result;
use tokio_core::reactor::Handle;
use tokio_core::io::Io;
use tokio_core::net::{TcpStream, TcpStreamNew};

mod xmpp_codec;
use xmpp_codec::*;


// type FullClient = sasl::Client<StartTLS<TCPConnection>>

#[derive(Debug)]
pub struct TcpClient {
    state: TcpClientState,
}

enum TcpClientState {
    Connecting(TcpStreamNew),
    SendStart(futures::sink::Send<XMPPStream<TcpStream>>),
    RecvStart(Option<XMPPStream<TcpStream>>),
    Established,
    Invalid,
}

impl std::fmt::Debug for TcpClientState {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let s = match *self {
            TcpClientState::Connecting(_) => "Connecting",
            TcpClientState::SendStart(_) => "SendStart",
            TcpClientState::RecvStart(_) => "RecvStart",
            TcpClientState::Established => "Established",
            TcpClientState::Invalid => "Invalid",
        };
        write!(fmt, "{}", s)
    }
}

impl TcpClient {
    pub fn connect(addr: &SocketAddr, handle: &Handle) -> Self {
        let tcp_stream_new = TcpStream::connect(addr, handle);
        TcpClient {
            state: TcpClientState::Connecting(tcp_stream_new),
        }
    }
}

impl Future for TcpClient {
    type Item = XMPPStream<TcpStream>;
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (new_state, result) = match self.state {
            TcpClientState::Connecting(ref mut tcp_stream_new) => {
                let tcp_stream = try_ready!(tcp_stream_new.poll());
                let xmpp_stream = tcp_stream.framed(XMPPCodec::new());
                let send = xmpp_stream.send(Packet::StreamStart);
                let new_state = TcpClientState::SendStart(send);
                (new_state, Ok(Async::NotReady))
            },
            TcpClientState::SendStart(ref mut send) => {
                let xmpp_stream = try_ready!(send.poll());
                let new_state = TcpClientState::RecvStart(Some(xmpp_stream));
                (new_state, Ok(Async::NotReady))
            },
            TcpClientState::RecvStart(ref mut opt_xmpp_stream) => {
                let mut xmpp_stream = opt_xmpp_stream.take().unwrap();
                match xmpp_stream.poll() {
                    Ok(Async::Ready(Some(events))) => println!("Recv start: {:?}", events),
                    Ok(Async::Ready(_)) => return Err(std::io::Error::from(ErrorKind::InvalidData)),
                    Ok(Async::NotReady) => {
                        *opt_xmpp_stream = Some(xmpp_stream);
                        return Ok(Async::NotReady);
                    },
                    Err(e) => return Err(e)
                };
                let new_state = TcpClientState::Established;
                (new_state, Ok(Async::Ready(xmpp_stream)))
            },
            TcpClientState::Established | TcpClientState::Invalid =>
                unreachable!(),
        };

        println!("Next state: {:?}", new_state);
        self.state = new_state;
	match result {
	    // by polling again, we register new future
	    Ok(Async::NotReady) => self.poll(),
	    result => result
	}
    }
}

#[cfg(test)]
mod tests {
    use tokio_core::reactor::Core;
    use futures::{Future, Stream};

    #[test]
    fn it_works() {
        use std::net::ToSocketAddrs;
        let addr = "[2a01:4f8:a0:33d0::5]:5222"
            .to_socket_addrs().unwrap()
            .next().unwrap();

        let mut core = Core::new().unwrap();
        let client = super::TcpClient::connect(
            &addr,
            &core.handle()
        ).and_then(|stream| {
            stream.for_each(|item| {
                Ok(println!("stream item: {:?}", item))
            })
        });
        core.run(client).unwrap();
    }

    // TODO: test truncated utf8
}
