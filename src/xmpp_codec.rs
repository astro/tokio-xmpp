use std;
use std::str::from_utf8;
use std::io::{Error, ErrorKind};
use std::collections::HashMap;
use tokio_core::io::{Codec, EasyBuf};
use xml;

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

impl Codec for XMPPCodec {
    type In = Vec<Packet>;
    type Out = Packet;

    fn decode(&mut self, buf: &mut EasyBuf) -> Result<Option<Self::In>, Error> {
        match from_utf8(buf.as_slice()) {
            Ok(s) =>
                self.parser.feed_str(s),
            Err(e) =>
                return Err(Error::new(ErrorKind::InvalidInput, e)),
        }

        let mut new_root = None;
        let mut results = Vec::new();
        for event in &mut self.parser {
            match &mut self.root {
                &mut None => {
                    // Expecting <stream:stream>
                    match event {
                        Ok(xml::Event::ElementStart(start_tag)) => {
                            new_root = Some(XMPPRoot::new(start_tag));
                            results.push(Packet::StreamStart);
                        },
                        Err(e) =>
                            results.push(Packet::Error(Box::new(e))),
                        _ =>
                            (),
                    }
                }

                &mut Some(ref mut root) => {
                    match root.handle_event(event) {
                        None => (),
                        Some(Ok(stanza)) => {
                            println!("stanza: {}", stanza);
                            results.push(Packet::Stanza(stanza));
                        },
                        Some(Err(e)) =>
                            results.push(Packet::Error(Box::new(e))),
                    };
                },
            }

            match new_root.take() {
                None => (),
                Some(root) => self.root = Some(root),
            }
        }

        if results.len() == 0 {
            Ok(None)
        } else {
            Ok(Some(results))
        }
    }

    fn encode(&mut self, msg: Self::Out, buf: &mut Vec<u8>) -> Result<(), Error> {
        match msg {
            Packet::StreamStart => {
                let mut write = |s: &str| {
                    buf.extend_from_slice(s.as_bytes());
                };

                write("<?xml version='1.0'?>\n");
                write("<stream:stream");
                write(" version='1.0'");
                write(" to='spaceboyz.net'");
                write(&format!(" xmlns='{}'", NS_CLIENT));
                write(&format!(" xmlns:stream='{}'", NS_STREAMS));
                write(">\n");

                Ok(())
            },
            // TODO: Implement all
            _ => Ok(())
        }
    }

    fn decode_eof(&mut self, _buf: &mut EasyBuf) -> Result<Self::In, Error> {
        Ok(vec!())
    }
}
