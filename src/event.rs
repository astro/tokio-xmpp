use minidom::Element;

/// High-level event on the Stream implemented by Client and Component
#[derive(Debug)]
pub enum Event {
    /// Stream is connected and initialized
    Online,
    /// Stream end
    Disconnected,
    /// Received stanza/nonza
    Stanza(Element),
}

impl Event {
    /// `Online` event?
    pub fn is_online(&self) -> bool {
        match *self {
            Event::Online => true,
            _ => false,
        }
    }

    /// `Stanza` event?
    pub fn is_stanza(&self, name: &str) -> bool {
        match *self {
            Event::Stanza(ref stanza) => stanza.name() == name,
            _ => false,
        }
    }

    /// If this is a `Stanza` event, get its data
    pub fn as_stanza(&self) -> Option<&Element> {
        match *self {
            Event::Stanza(ref stanza) => Some(stanza),
            _ => None,
        }
    }

    /// If this is a `Stanza` event, unwrap into its data
    pub fn into_stanza(self) -> Option<Element> {
        match self {
            Event::Stanza(stanza) => Some(stanza),
            _ => None,
        }
    }
}
