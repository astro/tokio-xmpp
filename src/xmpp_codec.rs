//! XML stream parser for XMPP

use std;
use std::default::Default;
use std::iter::FromIterator;
use std::cell::RefCell;
use std::rc::Rc;
use std::fmt::Write;
use std::str::from_utf8;
use std::io;
use std::collections::HashMap;
use std::collections::vec_deque::VecDeque;
use tokio_codec::{Encoder, Decoder};
use minidom::Element;
use xml5ever::tokenizer::{XmlTokenizer, TokenSink, Token, Tag, TagKind};
use xml5ever::interface::Attribute;
use bytes::{BytesMut, BufMut};
use quick_xml::Writer as EventWriter;
use crate::{ParserError, ParseError};

/// Anything that can be sent or received on an XMPP/XML stream
#[derive(Debug)]
pub enum Packet {
    /// `<stream:stream>` start tag
    StreamStart(HashMap<String, String>),
    /// A complete stanza or nonza
    Stanza(Element),
    /// Plain text (think whitespace keep-alive)
    Text(String),
    /// `</stream:stream>` closing tag
    StreamEnd,
}

type QueueItem = Result<Packet, ParserError>;

/// Parser state
struct ParserSink {
    // Ready stanzas, shared with XMPPCodec
    queue: Rc<RefCell<VecDeque<QueueItem>>>,
    // Parsing stack
    stack: Vec<Element>,
    ns_stack: Vec<HashMap<Option<String>, String>>,
}

impl ParserSink {
    pub fn new(queue: Rc<RefCell<VecDeque<QueueItem>>>) -> Self {
        ParserSink {
            queue,
            stack: vec![],
            ns_stack: vec![],
        }
    }

    fn push_queue(&self, pkt: Packet) {
        self.queue.borrow_mut().push_back(Ok(pkt));
    }

    fn push_queue_error(&self, e: ParserError) {
        self.queue.borrow_mut().push_back(Err(e));
    }

    /// Lookup XML namespace declaration for given prefix (or no prefix)
    fn lookup_ns(&self, prefix: &Option<String>) -> Option<&str> {
        for nss in self.ns_stack.iter().rev() {
            if let Some(ns) = nss.get(prefix) {
                return Some(ns);
            }
        }

        None
    }

    fn handle_start_tag(&mut self, tag: Tag) {
        let mut nss = HashMap::new();
        let is_prefix_xmlns = |attr: &Attribute| attr.name.prefix.as_ref()
            .map(|prefix| prefix.eq_str_ignore_ascii_case("xmlns"))
            .unwrap_or(false);
        for attr in &tag.attrs {
            match attr.name.local.as_ref() {
                "xmlns" => {
                    nss.insert(None, attr.value.as_ref().to_owned());
                },
                prefix if is_prefix_xmlns(attr) => {
                        nss.insert(Some(prefix.to_owned()), attr.value.as_ref().to_owned());
                    },
                _ => (),
            }
        }
        self.ns_stack.push(nss);

        let el = {
            let mut el_builder = Element::builder(tag.name.local.as_ref());
            if let Some(el_ns) = self.lookup_ns(
                &tag.name.prefix.map(|prefix|
                                     prefix.as_ref().to_owned())
            ) {
                el_builder = el_builder.ns(el_ns);
            }
            for attr in &tag.attrs {
                match attr.name.local.as_ref() {
                    "xmlns" => (),
                    _ if is_prefix_xmlns(attr) => (),
                    _ => {
                        el_builder = el_builder.attr(
                            attr.name.local.as_ref(),
                            attr.value.as_ref()
                        );
                    },
                }
            }
            el_builder.build()
        };

        if self.stack.is_empty() {
            let attrs = HashMap::from_iter(
                tag.attrs.iter()
                    .map(|attr| (attr.name.local.as_ref().to_owned(), attr.value.as_ref().to_owned()))
            );
            self.push_queue(Packet::StreamStart(attrs));
        }

        self.stack.push(el);
    }

    fn handle_end_tag(&mut self) {
        let el = self.stack.pop().unwrap();
        self.ns_stack.pop();

        match self.stack.len() {
            // </stream:stream>
            0 =>
                self.push_queue(Packet::StreamEnd),
            // </stanza>
            1 =>
                self.push_queue(Packet::Stanza(el)),
            len => {
                let parent = &mut self.stack[len - 1];
                parent.append_child(el);
            },
        }
    }
}

impl TokenSink for ParserSink {
    fn process_token(&mut self, token: Token) {
        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag =>
                    self.handle_start_tag(tag),
                TagKind::EndTag =>
                    self.handle_end_tag(),
                TagKind::EmptyTag => {
                    self.handle_start_tag(tag);
                    self.handle_end_tag();
                },
                TagKind::ShortTag =>
                    self.push_queue_error(ParserError::ShortTag),
            },
            Token::CharacterTokens(tendril) =>
                match self.stack.len() {
                    0 | 1 =>
                        self.push_queue(Packet::Text(tendril.into())),
                    len => {
                        let el = &mut self.stack[len - 1];
                        el.append_text_node(tendril);
                    },
                },
            Token::EOFToken =>
                self.push_queue(Packet::StreamEnd),
            Token::ParseError(s) => {
                // println!("ParseError: {:?}", s);
                self.push_queue_error(ParserError::Parse(ParseError(s)));
            },
            _ => (),
        }
    }

    // fn end(&mut self) {
    // }
}

/// Stateful encoder/decoder for a bytestream from/to XMPP `Packet`
pub struct XMPPCodec {
    /// Outgoing
    ns: Option<String>,
    /// Incoming
    parser: XmlTokenizer<ParserSink>,
    /// For handling incoming truncated utf8
    // TODO: optimize using  tendrils?
    buf: Vec<u8>,
    /// Shared with ParserSink
    queue: Rc<RefCell<VecDeque<QueueItem>>>,
}

impl XMPPCodec {
    /// Constructor
    pub fn new() -> Self {
        let queue = Rc::new(RefCell::new(VecDeque::new()));
        let sink = ParserSink::new(queue.clone());
        // TODO: configure parser?
        let parser = XmlTokenizer::new(sink, Default::default());
        XMPPCodec {
            ns: None,
            parser,
            queue,
            buf: vec![],
        }
    }
}

impl Default for XMPPCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for XMPPCodec {
    type Item = Packet;
    type Error = ParserError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let buf1: Box<AsRef<[u8]>> =
            if ! self.buf.is_empty() && ! buf.is_empty() {
                let mut prefix = std::mem::replace(&mut self.buf, vec![]);
                prefix.extend_from_slice(buf.take().as_ref());
                Box::new(prefix)
            } else {
                Box::new(buf.take())
            };
        let buf1 = buf1.as_ref().as_ref();
        match from_utf8(buf1) {
            Ok(s) => {
                if ! s.is_empty() {
                    // println!("<< {}", s);
                    let tendril = FromIterator::from_iter(s.chars());
                    self.parser.feed(tendril);
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
                // println!("error {} at {}/{} in {:?}", e, e.valid_up_to(), buf1.len(), buf1);
                return Err(ParserError::Utf8(e));
            },
        }

        match self.queue.borrow_mut().pop_front() {
            None => Ok(None),
            Some(result) =>
                result.map(|pkt| Some(pkt)),
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decode(buf)
    }
}

impl Encoder for XMPPCodec {
    type Item = Packet;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let remaining = dst.capacity() - dst.len();
        let max_stanza_size: usize = 2usize.pow(16);
        if remaining < max_stanza_size {
            dst.reserve(max_stanza_size - remaining);
        }

        match item {
            Packet::StreamStart(start_attrs) => {
                let mut buf = String::new();
                write!(buf, "<stream:stream").unwrap();
                for (name, value) in start_attrs {
                    write!(buf, " {}=\"{}\"", escape(&name), escape(&value))
                        .unwrap();
                    if name == "xmlns" {
                        self.ns = Some(value);
                    }
                }
                write!(buf, ">\n").unwrap();

                print!(">> {}", buf);
                write!(dst, "{}", buf)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
            },
            Packet::Stanza(stanza) => {
                stanza.write_to_inner(&mut EventWriter::new(WriteBytes::new(dst)))
                    .and_then(|_| {
                        // println!(">> {:?}", dst);
                        Ok(())
                    })
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{}", e)))
            },
            Packet::Text(text) => {
                write_text(&text, dst)
                    .and_then(|_| {
                        // println!(">> {:?}", dst);
                        Ok(())
                    })
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{}", e)))
            },
            // TODO: Implement all
            _ => Ok(())
        }
    }
}

/// Write XML-escaped text string
pub fn write_text<W: Write>(text: &str, writer: &mut W) -> Result<(), std::fmt::Error> {
    write!(writer, "{}", escape(text))
}

/// Copied from `RustyXML` for now
pub fn escape(input: &str) -> String {
    let mut result = String::with_capacity(input.len());

    for c in input.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '\'' => result.push_str("&apos;"),
            '"' => result.push_str("&quot;"),
            o => result.push(o)
        }
    }
    result
}

/// BytesMut impl only std::fmt::Write but not std::io::Write. The
/// latter trait is required for minidom's
/// `Element::write_to_inner()`.
struct WriteBytes<'a> {
    dst: &'a mut BytesMut,
}

impl<'a> WriteBytes<'a> {
    fn new(dst: &'a mut BytesMut) -> Self {
        WriteBytes { dst }
    }
}

impl<'a> std::io::Write for WriteBytes<'a> {
    fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, std::io::Error> {
        self.dst.put_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::result::Result<(), std::io::Error> {
        Ok(())
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
                if el.name() == "test"
                && el.text() == "ß"
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
                if el.name() == "test"
                && el.text() == "ß"
                => true,
            _ => false,
        });
    }

    /// By default, encode() only get's a BytesMut that has 8kb space reserved.
    #[test]
    fn test_large_stanza() {
        use std::io::Cursor;
        use futures::{Future, Sink};
        use tokio_codec::FramedWrite;
        let framed = FramedWrite::new(Cursor::new(vec![]), XMPPCodec::new());
        let mut text = "".to_owned();
        for _ in 0..2usize.pow(15) {
            text = text + "A";
        }
        let stanza = Element::builder("message")
            .append(
                Element::builder("body")
                    .append(&text)
                    .build()
            )
            .build();
        let framed = framed.send(Packet::Stanza(stanza))
            .wait()
            .expect("send");
        assert_eq!(framed.get_ref().get_ref(), &("<message><body>".to_owned() + &text + "</body></message>").as_bytes());
    }
}
