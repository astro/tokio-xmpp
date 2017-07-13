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
    buf: Vec<u8>,
}

impl XMPPCodec {
    pub fn new() -> Self {
        XMPPCodec {
            parser: xml::Parser::new(),
            root: None,
            buf: vec![],
        }
    }
}

impl Decoder for XMPPCodec {
    type Item = Packet;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let buf1: Box<AsRef<[u8]>> =
            if self.buf.len() > 0 && buf.len() > 0 {
                let mut prefix = std::mem::replace(&mut self.buf, vec![]);
                prefix.extend_from_slice(buf.take().as_ref());
                Box::new(prefix)
            } else {
                Box::new(buf.take())
            };
        let buf1 = buf1.as_ref().as_ref();
        match from_utf8(buf1) {
            Ok(s) => {
                if s.len() > 0 {
                    println!("<< {}", s);
                    self.parser.feed_str(s);
                }
            },
            // Remedies for truncated utf8
            Err(e) if e.valid_up_to() >= buf1.len() - 3 => {
                // Prepare all the valid data
                let mut b = BytesMut::with_capacity(e.valid_up_to());
                b.put(&buf1[0..e.valid_up_to()]);

                // Retry
                let result = self.decode(&mut b);

                // Keep the tail back in
                self.buf.extend_from_slice(&buf1[e.valid_up_to()..]);

                return result;
            },
            Err(e) => {
                println!("error {} at {}/{} in {:?}", e, e.valid_up_to(), buf1.len(), buf1);
                return Err(Error::new(ErrorKind::InvalidInput, e));
            },
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

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_stream_start() {
        let mut c = XMPPCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put(r"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });
    }

    #[test]
    fn test_truncated_stanza() {
        let mut c = XMPPCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put(r"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });

        b.clear();
        b.put(r"<test>ß</test");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(None) => true,
            _ => false,
        });

        b.clear();
        b.put(r">");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::Stanza(ref el)))
                if el.name == "test"
                && el.content_str() == "ß"
                => true,
            _ => false,
        });
    }

    #[test]
    fn test_truncated_utf8() {
        let mut c = XMPPCodec::new();
        let mut b = BytesMut::with_capacity(1024);
        b.put(r"<?xml version='1.0'?><stream:stream xmlns:stream='http://etherx.jabber.org/streams' version='1.0' xmlns='jabber:client'>");
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::StreamStart(_))) => true,
            _ => false,
        });

        b.clear();
        b.put(&b"<test>\xc3"[..]);
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(None) => true,
            _ => false,
        });

        b.clear();
        b.put(&b"\x9f</test>"[..]);
        let r = c.decode(&mut b);
        assert!(match r {
            Ok(Some(Packet::Stanza(ref el)))
                if el.name == "test"
                && el.content_str() == "ß"
                => true,
            _ => false,
        });
    }
}
