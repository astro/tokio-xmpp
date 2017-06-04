use std;
use std::fmt::Write;
use std::str::from_utf8;
use std::io::{Error, ErrorKind};
use std::collections::HashMap;
use tokio_io::codec::{Framed, Encoder, Decoder};
use xml;
use bytes::*;

const NS_XMLNS: &'static str = "http://www.w3.org/2000/xmlns/";
const NS_STREAMS: &'static str = "http://etherx.jabber.org/streams";
const NS_CLIENT: &'static str = "jabber:client";

struct XMPPRoot {
    builder: xml::ElementBuilder,
    pub attributes: HashMap<(String, Option<String>), String>,
}

impl XMPPRoot {
    fn new(root: xml::StartTag) -> Self {
        let mut builder = xml::ElementBuilder::new();
        let mut attributes = HashMap::new();
        println!("root attributes: {:?}", root.attributes);
        for (name_ns, value) in root.attributes {
            match name_ns {
                (ref name, None) if name == "xmlns" =>
                    builder.set_default_ns(value),
                (ref prefix, Some(ref ns)) if ns == NS_XMLNS =>
                    builder.define_prefix(prefix.to_owned(), value),
                _ => {
                    attributes.insert(name_ns, value);
                },
            }
        }

        XMPPRoot {
            builder: builder,
            attributes: attributes,
        }
    }

    fn handle_event(&mut self, event: Result<xml::Event, xml::ParserError>)
                    -> Option<Result<xml::Element, xml::BuilderError>> {
        self.builder.handle_event(event)
    }
}

#[derive(Debug)]
pub enum Packet {
    Error(Box<std::error::Error>),
    StreamStart,
    Stanza(xml::Element),
    StreamEnd,
}

pub type XMPPStream<T> = Framed<T, XMPPCodec>;

pub struct XMPPCodec {
    parser: xml::Parser,
    root: Option<XMPPRoot>,
}

impl XMPPCodec {
    pub fn new() -> Self {
        XMPPCodec {
            parser: xml::Parser::new(),
            root: None,
        }
    }
}

impl Decoder for XMPPCodec {
    type Item = Packet;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        println!("XMPPCodec.decode {:?}", buf.len());
        match from_utf8(buf.take().as_ref()) {
            Ok(s) =>
                self.parser.feed_str(s),
            Err(e) =>
                return Err(Error::new(ErrorKind::InvalidInput, e)),
        }

        let mut new_root: Option<XMPPRoot> = None;
        let mut result = None;
        for event in &mut self.parser {
            match self.root {
                None => {
                    // Expecting <stream:stream>
                    match event {
                        Ok(xml::Event::ElementStart(start_tag)) => {
                            self.root = Some(XMPPRoot::new(start_tag));
                            result = Some(Packet::StreamStart);
                            break
                        },
                        Err(e) => {
                            result = Some(Packet::Error(Box::new(e)));
                            break
                        },
                        _ =>
                            (),
                    }
                }

                Some(ref mut root) => {
                    match root.handle_event(event) {
                        None => (),
                        Some(Ok(stanza)) => {
                            println!("stanza: {}", stanza);
                            result = Some(Packet::Stanza(stanza));
                            break
                        },
                        Some(Err(e)) => {
                            result = Some(Packet::Error(Box::new(e)));
                            break
                        }
                    };
                },
            }

            match new_root.take() {
                None => (),
                Some(root) => self.root = Some(root),
            }
        }

        Ok(result)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Error> {
        self.decode(buf)
    }
}

impl Encoder for XMPPCodec {
    type Item = Packet;
    type Error = Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            Packet::StreamStart => {
                write!(dst,
                       "<?xml version='1.0'?>\n
<stream:stream version='1.0' to='spaceboyz.net' xmlns='{}' xmlns:stream='{}'>\n",
                       NS_CLIENT, NS_STREAMS)
                    .map_err(|_| Error::from(ErrorKind::WriteZero))
            },
            // TODO: Implement all
            _ => Ok(())
        }
    }
}
