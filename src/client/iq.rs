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

/// `(stanza.from, atanza.to)`
type IqIdentification = (String, String);

pub trait IdentifiesIq {
    fn identify(self) -> Option<IqIdentification>;
}

/// For any `(String, String)`
impl IdentifiesIq for IqIdentification {
    fn identify(self) -> Option<IqIdentification> {
        Some(self)
    }
}

/// Extracts from stanza
impl<'a> IdentifiesIq for &'a Element {
    fn identify(self) -> Option<IqIdentification> {
        self.attr("id").and_then(
            |id| self.attr("from").map(
                |from| (from.to_owned(), id.to_owned())
            ))
    }
}

/// An `FnOnce` callback would give more freedom to users Check back
/// at a later Rust version.
type Callback = FnMut(Result<&Element, &Element>) -> Box<Future<Item = (), Error = String>>;

pub struct Tracker {
    pending: HashMap<IqIdentification, Box<Callback>>,
}

impl Tracker {
    pub fn new() -> Self {
        Tracker {
            pending: HashMap::new(),
        }
    }

    pub fn has<K: IdentifiesIq>(&self, key: K) -> bool {
        match key.identify() {
            Some(ident) =>
                self.pending.get(&ident).is_some(),
            None =>
                false,
        }
    }

    pub fn insert<K: IdentifiesIq>(&mut self, key: K, callback: Box<Callback>) {
        match key.identify() {
            Some(ident) => {
                self.pending.insert(ident, callback);
            },
            None =>
                (),
        }
    }

    pub fn trigger(&mut self, stanza: &Element) -> Box<Future<Item = bool, Error = String>> {
        let key = stanza.identify();

        let arg = if stanza.name() == "iq" && stanza.attr("type") == Some("result") {
            Some(Ok(stanza))
        } else if stanza.name() == "iq" && stanza.attr("type") == Some("error") {
            Some(Err(stanza))
        } else {
            None
        };
        arg.and_then(
            |arg| {
                key.and_then(
                    |key| self.remove(key).map(
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

    pub fn remove<K: IdentifiesIq>(&mut self, key: K) -> Option<Box<Callback>> {
        key.identify().and_then(
            |ident| self.pending.remove(&ident)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minidom::Element;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn test_has_id() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        tracker.insert(ident.clone(), Box::new(|_| Box::new(future::ok(()))));
        assert!(tracker.has(ident));
    }

    #[test]
    fn test_remove_existing() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        tracker.insert(ident.clone(), Box::new(|_| Box::new(future::ok(()))));
        assert!(tracker.remove(ident).is_some());
    }

    #[test]
    fn test_remove_missing() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        tracker.insert(ident, Box::new(|_| Box::new(future::ok(()))));
        let ident2 = ("b@example.com".to_owned(), "xyz".to_owned());
        assert!(tracker.remove(ident2).is_none());
    }

    #[test]
    fn test_trigger_simple_result() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let callback_called = Rc::new(RefCell::new(false));
        let callback_called_ = callback_called.clone();
        tracker.insert(ident, Box::new(move |iq_stanza| {
            assert!(iq_stanza.is_ok());
            *callback_called_.borrow_mut() = true;
            Box::new(future::ok(()))
        }));
        let iq = Element::builder("iq")
            .attr("from", "a@example.com")
            .attr("id", "abc")
            .attr("type", "result")
            .build();
        assert_eq!(tracker.trigger(&iq).wait(), Ok(true));
        assert_eq!(*callback_called.borrow(), true);
    }

    #[test]
    fn test_trigger_simple_error() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let callback_called = Rc::new(RefCell::new(false));
        let callback_called_ = callback_called.clone();
        tracker.insert(ident, Box::new(move |iq_stanza| {
            assert!(iq_stanza.is_err());
            *callback_called_.borrow_mut() = true;
            Box::new(future::ok(()))
        }));
        let iq = Element::builder("iq")
            .attr("from", "a@example.com")
            .attr("id", "abc")
            .attr("type", "error")
            .build();
        assert_eq!(tracker.trigger(&iq).wait(), Ok(true));
        assert_eq!(*callback_called.borrow(), true);
    }

    #[test]
    fn test_trigger_wrong_id() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let callback_called = Rc::new(RefCell::new(false));
        let callback_called_ = callback_called.clone();
        tracker.insert(ident, Box::new(move |iq_stanza| {
            *callback_called_.borrow_mut() = true;
            Box::new(future::ok(()))
        }));
        let iq = Element::builder("iq")
            .attr("from", "a@example.com")
            .attr("id", "xyz")
            .attr("type", "result")
            .build();
        assert_eq!(tracker.trigger(&iq).wait(), Ok(false));
        assert_eq!(*callback_called.borrow(), false);
    }

    #[test]
    fn test_trigger_wrong_from() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let callback_called = Rc::new(RefCell::new(false));
        let callback_called_ = callback_called.clone();
        tracker.insert(ident, Box::new(move |iq_stanza| {
            *callback_called_.borrow_mut() = true;
            Box::new(future::ok(()))
        }));
        let iq = Element::builder("iq")
            .attr("from", "b@example.com")
            .attr("id", "abc")
            .attr("type", "result")
            .build();
        assert_eq!(tracker.trigger(&iq).wait(), Ok(false));
        assert_eq!(*callback_called.borrow(), false);
    }
}
