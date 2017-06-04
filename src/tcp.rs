use std::fmt;
use std::net::SocketAddr;
use std::io::{Error, ErrorKind};
use futures::{Future, Sink, Poll, Async};
use futures::stream::Stream;
use futures::sink;
use tokio_core::reactor::Handle;
use tokio_core::net::{TcpStream, TcpStreamNew};

use super::{XMPPStream, XMPPCodec, Packet};


#[derive(Debug)]
pub struct TcpClient {
    state: TcpClientState,
}

enum TcpClientState {
    Connecting(TcpStreamNew),
    SendStart(sink::Send<XMPPStream<TcpStream>>),
    RecvStart(Option<XMPPStream<TcpStream>>),
    Established,
}

impl fmt::Debug for TcpClientState {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let s = match *self {
            TcpClientState::Connecting(_) => "Connecting",
            TcpClientState::SendStart(_) => "SendStart",
            TcpClientState::RecvStart(_) => "RecvStart",
            TcpClientState::Established => "Established",
        };
        try!(write!(fmt, "{}", s));
        Ok(())
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
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (new_state, result) = match self.state {
            TcpClientState::Connecting(ref mut tcp_stream_new) => {
                let tcp_stream = try_ready!(tcp_stream_new.poll());
                let xmpp_stream = XMPPCodec::frame_stream(tcp_stream);
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
                    Ok(Async::Ready(Some(Packet::StreamStart))) => println!("Recv start!"),
                    Ok(Async::Ready(_)) => return Err(Error::from(ErrorKind::InvalidData)),
                    Ok(Async::NotReady) => {
                        *opt_xmpp_stream = Some(xmpp_stream);
                        return Ok(Async::NotReady);
                    },
                    Err(e) => return Err(e)
                };
                let new_state = TcpClientState::Established;
                (new_state, Ok(Async::Ready(xmpp_stream)))
            },
            TcpClientState::Established =>
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
