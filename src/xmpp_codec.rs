use std;
use std::fmt::Write;
use std::str::from_utf8;
use std::io::{Error, ErrorKind};
use std::collections::HashMap;
use tokio_io::codec::{Encoder, Decoder};
use xml;
use bytes::*;

const NS_XMLNS: &'static str = "http://www.w3.org/2000/xmlns/";

pub type Attributes = HashMap<(String, Option<String>), String>;

struct XMPPRoot {
    builder: xml::ElementBuilder,
    pub attributes: Attributes,
}

impl XMPPRoot {
    fn new(root: xml::StartTag) -> Self {
        let mut builder = xml::ElementBuilder::new();
        let mut attributes = HashMap::new();
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
    StreamStart(HashMap<String, String>),
    Stanza(xml::Element),
    Text(String),
    StreamEnd,
}

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
        match from_utf8(buf.take().as_ref()) {
            Ok(s) => {
                if s.len() > 0 {
                    println!("<< {}", s);
                    self.parser.feed_str(s);
                }
            },
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
                            let mut attrs: HashMap<String, String> = HashMap::new();
                            for (&(ref name, _), value) in &start_tag.attributes {
                                attrs.insert(name.to_owned(), value.to_owned());
                            }
                            result = Some(Packet::StreamStart(attrs));
                            self.root = Some(XMPPRoot::new(start_tag));
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
                            // Emit the stanza
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
            Packet::StreamStart(start_attrs) => {
                let mut buf = String::new();
                write!(buf, "<stream:stream").unwrap();
                for (ref name, ref value) in &start_attrs {
                    write!(buf, " {}=\"{}\"", xml::escape(&name), xml::escape(&value))
                        .unwrap();
                }
                write!(buf, ">\n").unwrap();

                print!(">> {}", buf);
                write!(dst, "{}", buf)
            },
            Packet::Stanza(stanza) => {
                println!(">> {}", stanza);
                write!(dst, "{}", stanza)
            },
            Packet::Text(text) => {
                let escaped = xml::escape(&text);
                println!(">> {}", escaped);
                write!(dst, "{}", escaped)
            },
            // TODO: Implement all
            _ => Ok(())
        }
        .map_err(|_| Error::from(ErrorKind::InvalidInput))
    }
}
