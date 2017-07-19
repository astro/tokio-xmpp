use minidom::Element;

#[derive(Debug)]
pub enum Event {
    Online,
    Disconnected,
    Stanza(Element),
}

impl Event {
    pub fn is_online(&self) -> bool {
        match self {
            &Event::Online => true,
            _ => false,
        }
    }

    pub fn is_stanza(&self, name: &str) -> bool {
        match self {
            &Event::Stanza(ref stanza) => stanza.name() == name,
            _ => false,
        }
    }

    pub fn as_stanza(&self) -> Option<&Element> {
        match self {
            &Event::Stanza(ref stanza) => Some(stanza),
            _ => None,
        }
    }

    pub fn into_stanza(self) -> Option<Element> {
        match self {
            Event::Stanza(stanza) => Some(stanza),
            _ => None,
        }
    }
}
