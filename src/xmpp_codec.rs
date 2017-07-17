use std;
use std::default::Default;
use std::iter::FromIterator;
use std::cell::RefCell;
use std::rc::Rc;
use std::fmt::Write;
use std::str::from_utf8;
use std::io::{Error, ErrorKind};
use std::collections::HashMap;
use std::collections::vec_deque::VecDeque;
use tokio_io::codec::{Encoder, Decoder};
use minidom::Element;
use xml5ever::tokenizer::{XmlTokenizer, TokenSink, Token, Tag, TagKind};
use bytes::*;

// const NS_XMLNS: &'static str = "http://www.w3.org/2000/xmlns/";

#[derive(Debug)]
pub enum Packet {
    Error(Box<std::error::Error>),
    StreamStart(HashMap<String, String>),
    Stanza(Element),
    Text(String),
    StreamEnd,
}

struct ParserSink {
    // Ready stanzas
    queue: Rc<RefCell<VecDeque<Packet>>>,
    // Parsing stack
    stack: Vec<Element>,
}

impl ParserSink {
    pub fn new(queue: Rc<RefCell<VecDeque<Packet>>>) -> Self {
        ParserSink {
            queue,
            stack: vec![],
        }
    }

    fn push_queue(&self, pkt: Packet) {
        println!("push: {:?}", pkt);
        self.queue.borrow_mut().push_back(pkt);
    }

    fn handle_start_tag(&mut self, tag: Tag) {
        let el = tag_to_element(&tag);
        self.stack.push(el);
    }

    fn handle_end_tag(&mut self) {
        let el = self.stack.pop().unwrap();
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

fn tag_to_element(tag: &Tag) -> Element {
    let el_builder = Element::builder(tag.name.local.as_ref())
        .ns(tag.name.ns.as_ref());
    let el_builder = tag.attrs.iter().fold(
        el_builder,
        |el_builder, attr| el_builder.attr(
            attr.name.local.as_ref(),
            attr.value.as_ref()
        )
    );
    el_builder.build()
}

impl TokenSink for ParserSink {
    fn process_token(&mut self, token: Token) {
        println!("Token: {:?}", token);
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
                    self.push_queue(Packet::Error(Box::new(Error::new(ErrorKind::InvalidInput, "ShortTag")))),
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
                println!("ParseError: {:?}", s);
                self.push_queue(Packet::Error(Box::new(Error::new(ErrorKind::InvalidInput, (*s).to_owned()))))
            },
            _ => (),
        }
    }

    // fn end(&mut self) {
    // }
}

pub struct XMPPCodec {
    parser: XmlTokenizer<ParserSink>,
    // For handling truncated utf8
    buf: Vec<u8>,
    queue: Rc<RefCell<VecDeque<Packet>>>,
}

impl XMPPCodec {
    pub fn new() -> Self {
        let queue = Rc::new(RefCell::new((VecDeque::new())));
        let sink = ParserSink::new(queue.clone());
        // TODO: configure parser?
        let parser = XmlTokenizer::new(sink, Default::default());
        XMPPCodec {
            parser,
            queue,
            buf: vec![],
        }
    }
}

impl Decoder for XMPPCodec {
    type Item = Packet;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        println!("decode {} bytes", buf.len());
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
                println!("error {} at {}/{} in {:?}", e, e.valid_up_to(), buf1.len(), buf1);
                return Err(Error::new(ErrorKind::InvalidInput, e));
            },
        }

        let result = self.queue.borrow_mut().pop_front();
        Ok(result)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decode(buf)
    }
}

impl Encoder for XMPPCodec {
    type Item = Packet;
    type Error = Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        println!("encode {:?}", item);
        match item {
            Packet::StreamStart(start_attrs) => {
                let mut buf = String::new();
                write!(buf, "<stream:stream").unwrap();
                for (ref name, ref value) in &start_attrs {
                    write!(buf, " {}=\"{}\"", escape(&name), escape(&value))
                        .unwrap();
                }
                write!(buf, ">\n").unwrap();

                print!(">> {}", buf);
                write!(dst, "{}", buf)
                    .map_err(|_| Error::from(ErrorKind::InvalidInput))
            },
            Packet::Stanza(stanza) => {
                println!(">> {:?}", stanza);
                let mut root_ns = None;  // TODO
                stanza.write_to_inner(&mut dst.clone().writer(), &mut root_ns)
                    .map_err(|_| Error::from(ErrorKind::InvalidInput))
            },
            Packet::Text(text) => {
                let escaped = escape(&text);
                println!(">> {}", escaped);
                write!(dst, "{}", escaped)
                    .map_err(|_| Error::from(ErrorKind::InvalidInput))
            },
            // TODO: Implement all
            _ => Ok(())
        }
    }
}

/// Copied from RustyXML for now
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
}
