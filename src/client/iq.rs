use std::io;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use futures::*;
use tokio_io::{AsyncRead, AsyncWrite};
use rand::{thread_rng, Rng};
use minidom::Element;

use xmpp_codec::*;
use xmpp_stream::*;

pub fn generate_id(len: usize) -> String {
    thread_rng().gen_ascii_chars().take(len).collect()
}

/// An `FnOnce` callback would give more freedom to users Check back
/// at a later Rust version.
type Callback = FnMut(Result<Element, Element>) -> Box<Future<Item = (), Error = String>>;

// TODO: add identification by (from, id)
pub struct Tracker {
    pending: HashMap<String, Box<Callback>>,
}

impl Tracker {
    pub fn new() -> Self {
        Tracker {
            pending: HashMap::new(),
        }
    }

    pub fn has_id(&self, id: &str) -> bool {
        self.pending.get(id).is_some()
    }
    
    pub fn insert(&mut self, id: String, callback: Box<Callback>) {
        self.pending.insert(id, callback);
    }

    // TODO: &Element
    pub fn trigger(&mut self, stanza: Element) -> Box<Future<Item = bool, Error = String>> {
        let arg = if stanza.name() == "iq" && stanza.attr("type") == Some("result") {
            Some(Ok(stanza))
        } else if stanza.name() == "iq" && stanza.attr("type") == Some("error") {
            Some(Err(stanza))
        } else {
            None
        };
        arg.and_then(
            |arg| {
                let get_id = |stanza: &Element| stanza.attr("id").map(|id| id.to_owned());
                let id: Option<String> =
                    arg.as_ref()
                    .map(&get_id)
                    .unwrap_or_else(&get_id);
                id.and_then(
                    |id| self.remove(&id).map(
                        |mut callback: Box<Callback>| {
                            let result: Box<Future<Item = bool, Error = String>> =
                                Box::new(
                                    callback(arg)
                                        .and_then(|_| future::ok(true))
                                );
                            result
                        })
                )
            }).unwrap_or_else(
            || Box::new(future::ok(false))
        )
    }

    pub fn remove(&mut self, id: &str) -> Option<Box<Callback>> {
        self.pending.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minidom::Element;

    #[test]
    fn test_has_id() {
        let mut tracker = Tracker::new();
        tracker.insert("abc".to_owned(), Box::new(|_| Box::new(future::ok(()))));
        assert!(tracker.has_id("abc"));
    }

    #[test]
    fn test_trigger_simple_result() {
        let mut tracker = Tracker::new();
        tracker.insert("abc".to_owned(), Box::new(|_| Box::new(future::ok(()))));
        let iq = Element::builder("iq")
            .attr("from", "a@example.com")
            .attr("id", "abc")
            .attr("type", "result")
            .build();
        assert_eq!(tracker.trigger(iq).wait(), Ok(true));
        // TODO: verify result
    }

    #[test]
    fn test_trigger_wrong_id() {
        let mut tracker = Tracker::new();
        tracker.insert("abc".to_owned(), Box::new(|_| Box::new(future::ok(()))));
        let iq = Element::builder("iq")
            .attr("from", "a@example.com")
            .attr("id", "xyz")
            .attr("type", "result")
            .build();
        assert_eq!(tracker.trigger(iq).wait(), Ok(false));
    }
}
