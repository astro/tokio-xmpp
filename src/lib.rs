extern crate futures;
extern crate tokio_core;
extern crate xml;

use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::io::ErrorKind;
use futures::{Future, BoxFuture, Sink, Poll};
use futures::stream::{Stream, iter};
use futures::future::result;
use tokio_core::reactor::Handle;
use tokio_core::io::Io;
use tokio_core::net::TcpStream;

mod xmpp_codec;
use xmpp_codec::*;


// type FullClient = sasl::Client<StartTLS<TCPConnection>>

type Event = ();
type Error = std::io::Error;

struct TCPStream {
    source: Box<Stream<Item=Event, Error=std::io::Error>>,
    sink: Arc<Box<futures::stream::SplitSink<tokio_core::io::Framed<tokio_core::net::TcpStream, xmpp_codec::XMPPCodec>>>>,
}

impl TCPStream {
    pub fn connect(addr: &SocketAddr, handle: &Handle) -> BoxFuture<Arc<TCPStream>, std::io::Error> {
        TcpStream::connect(addr, handle)
            .and_then(|stream| {
                let (sink, source) = stream.framed(XMPPCodec::new())
                // .framed(UTF8Codec::new())
                    .split();
                
                sink.send(Packet::StreamStart)
                    .and_then(|sink| result(Ok((Arc::new(Box::new(sink)), source))))
            })
            .and_then(|(sink, source)| {
                let sink1 = sink.clone();
                let source = source
                    .map(|items| iter(items.into_iter().map(Ok)))
                    .flatten()
                    .filter_map(move |pkt| Self::process_packet(pkt, &sink1))
                // .for_each(|ev| {
                //     match ev {
                //         Packet::Stanza
                //         _ => (),
                //     }
                //     Ok(println!("xmpp: {:?}", ev))
                // })
                // .boxed();
                    ;
                result(Ok(Arc::new(TCPStream {
                    source: Box::new(source),
                    sink: sink,
                })))
            }).boxed()
            //.map_err(|e| std::io::Error::new(ErrorKind::Other, e));
    }

    fn process_packet<S>(pkt: Packet, sink: &Arc<S>) -> Option<Event>
        where S: Sink<SinkItem=Packet, SinkError=std::io::Error> {

        println!("pkt: {:?}", pkt);
        None
    }
}

struct ClientStream {
    inner: TCPStream,
}

impl ClientStream {
    pub fn connect(jid: &str, password: &str, handle: &Handle) -> Box<Future<Item=Self, Error=std::io::Error>> {
        let addr = "[2a01:4f8:a0:33d0::5]:5222"
            .to_socket_addrs().unwrap()
            .next().unwrap();
        let stream =
            TCPStream::connect(&addr, handle)
            .and_then(|stream| {
                Ok(ClientStream {
                    inner: stream
                })
            });
        Box::new(stream)
    }
}

#[cfg(test)]
mod tests {
    use tokio_core::reactor::Core;

    #[test]
    fn it_works() {
        let mut core = Core::new().unwrap();
        let client = super::ClientStream::connect(
            "astro@spaceboyz.net",
            "...",
            &core.handle()
        ).and_then(|stream| {
            stream.inner.source.boxed().for_each(|item| {
                Ok(println!("stream item: {:?}", item))
            })
        }).boxed();
        core.run(client).unwrap();
    }

    // TODO: test truncated utf8
}
