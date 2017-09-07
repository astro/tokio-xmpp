use std::mem::replace;
use std::cell::RefCell;
use std::rc::Rc;
use std::collections::HashMap;
use futures::{Future, Async, Poll};
use rand::{thread_rng, Rng};
use minidom::Element;


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


type Futures = Rc<RefCell<HashMap<IqIdentification, Rc<RefCell<IqState>>>>>;
pub struct Tracker {
    futures: Futures,
}

impl Tracker {
    pub fn new() -> Self {
        println!("Tracker.new");
        Tracker {
            futures: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn has<K: IdentifiesIq>(&self, key: K) -> bool {
        match key.identify() {
            Some(ident) =>
                self.futures.borrow().get(&ident).is_some(),
            None =>
                false,
        }
    }

    pub fn insert<K: IdentifiesIq>(&mut self, key: K) -> IqFuture {
        let futures = self.futures.clone();
        match key.identify() {
            Some(ident) => {
                let state = Rc::new(RefCell::new(IqState::Pending));
                self.futures.borrow_mut().insert(ident.clone(), state.clone());
                let future = IqFuture::new(futures, Some(ident), state);
                future
            },
            None =>
                IqFuture::new(futures, None, Rc::new(RefCell::new(IqState::Failed))),
        }
    }

    fn element_to_state(stanza: Element) -> Result<IqState, Element> {
        if stanza.name() != "iq" {
            return Err(stanza);
        }

        if stanza.attr("type") == Some("result") {
            return Ok(IqState::Result(stanza));
        } else if stanza.attr("type") == Some("error") {
            return Ok(IqState::Error(stanza));
        } else {
            return Err(stanza);
        }
    }

    pub fn trigger(&mut self, stanza: Element) -> Result<(), Element> {
        let ident = match stanza.identify() {
            Some(ident) => ident,
            None => return Err(stanza),
        };
        let state = match self.remove(ident) {
            Some(state) => state,
            None => return Err(stanza),
        };

        *state.borrow_mut() = Self::element_to_state(stanza)?;
        Ok(())
    }

    pub fn remove<K: IdentifiesIq>(&mut self, key: K) -> Option<Rc<RefCell<IqState>>> {
        key.identify().and_then(
            |ident| self.futures.borrow_mut().remove(&ident)
        )
    }
}

impl Drop for Tracker {
    fn drop(&mut self) {
        println!("Tracker.drop");
        let mut futures = self.futures.borrow_mut();
        for (_, state) in futures.drain() {
            *state.borrow_mut() = IqState::Failed;
        }
    }
}

#[derive(Debug)]
pub enum IqState {
    Pending,
    Result(Element),
    Error(Element),
    Failed,
}

pub struct IqFuture {
    ident: IqIdentification,
    state: Rc<RefCell<IqState>>,
    futures: Futures,
}

impl IqFuture {
    fn new(futures: Futures, ident: Option<IqIdentification>, state: Rc<RefCell<IqState>>) -> Self {
        let ident = match ident {
            Some(ident) =>
                ident,
            None => {
                *state.borrow_mut() = IqState::Failed;
                ("".to_owned(), "".to_owned())
            },
        };
        IqFuture {
            ident,
            state,
            futures,
        }
    }
}

impl Drop for IqFuture {
    fn drop(&mut self) {
        self.futures.borrow_mut().remove(&self.ident);
    }
}

impl Future for IqFuture {
    type Item = Element;
    type Error = Option<Element>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut ref_state = self.state.borrow_mut();
        let state = replace(&mut *ref_state, IqState::Failed);
        match state {
            IqState::Pending => {
                *ref_state = IqState::Pending;
                Ok(Async::NotReady)
            },
            IqState::Result(stanza) =>
                Ok(Async::Ready(stanza)),
            IqState::Error(stanza) =>
                Err(Some(stanza)),
            IqState::Failed =>
                Err(None),
        }
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
        let iq = tracker.insert(ident.clone());
        assert!(tracker.has(ident));
    }

    #[test]
    fn test_remove_existing() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let iq = tracker.insert(ident.clone());
        assert!(tracker.remove(ident).is_some());
    }

    #[test]
    fn test_remove_missing() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let iq = tracker.insert(ident);
        let ident2 = ("b@example.com".to_owned(), "xyz".to_owned());
        assert!(tracker.remove(ident2).is_none());
    }

    #[test]
    fn test_trigger_simple_result() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let iq = tracker.insert(ident);

        let stanza = Element::builder("iq")
            .attr("from", "a@example.com")
            .attr("id", "abc")
            .attr("type", "result")
            .build();
        assert_eq!(tracker.trigger(stanza.clone()), Ok(()));

        assert_eq!(iq.wait(), Ok(stanza));
    }

    #[test]
    fn test_trigger_simple_error() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let iq = tracker.insert(ident);
        let stanza = Element::builder("iq")
            .attr("from", "a@example.com")
            .attr("id", "abc")
            .attr("type", "error")
            .build();
        assert_eq!(tracker.trigger(stanza.clone()), Ok(()));

        assert_eq!(iq.wait(), Err(Some(stanza)));
    }

    #[test]
    fn test_trigger_wrong_id() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let mut iq = tracker.insert(ident);
        let stanza = Element::builder("iq")
            .attr("from", "a@example.com")
            .attr("id", "xyz")
            .attr("type", "result")
            .build();
        let returned_stanza = tracker.trigger(stanza.clone())
            .unwrap_err();
        assert_eq!(stanza, returned_stanza);

        assert_eq!(iq.poll(), Ok(Async::NotReady));
    }

    #[test]
    fn test_trigger_wrong_from() {
        let mut tracker = Tracker::new();
        let ident = ("a@example.com".to_owned(), "abc".to_owned());
        let mut iq = tracker.insert(ident);
        let stanza = Element::builder("iq")
            .attr("from", "b@example.com")
            .attr("id", "abc")
            .attr("type", "result")
            .build();
        let returned_stanza = tracker.trigger(stanza.clone())
            .unwrap_err();
        assert_eq!(stanza, returned_stanza);

        assert_eq!(iq.poll(), Ok(Async::NotReady));
    }
}
