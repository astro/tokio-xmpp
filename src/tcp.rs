use std::net::SocketAddr;
use std::io::Error;
use futures::{Future, Poll, Async};
use tokio_core::reactor::Handle;
use tokio_core::net::{TcpStream, TcpStreamNew};

use xmpp_stream::*;
use stream_start::StreamStart;

pub struct TcpClient {
    state: TcpClientState,
}

enum TcpClientState {
    Connecting(TcpStreamNew),
    Start(StreamStart<TcpStream>),
    Established,
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
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (new_state, result) = match self.state {
            TcpClientState::Connecting(ref mut tcp_stream_new) => {
                let tcp_stream = try_ready!(tcp_stream_new.poll());
                let start = XMPPStream::from_stream(tcp_stream, "spaceboyz.net".to_owned());
                let new_state = TcpClientState::Start(start);
                (new_state, Ok(Async::NotReady))
            },
            TcpClientState::Start(ref mut start) => {
                let xmpp_stream = try_ready!(start.poll());
                let new_state = TcpClientState::Established;
                (new_state, Ok(Async::Ready(xmpp_stream)))
            },
            TcpClientState::Established =>
                unreachable!(),
        };

        self.state = new_state;
	match result {
	    // by polling again, we register new future
	    Ok(Async::NotReady) => self.poll(),
	    result => result
	}
    }
}
