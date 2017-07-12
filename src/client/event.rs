use xml;

#[derive(Debug)]
pub enum Event {
    Online,
    Disconnected,
    Stanza(xml::Element),
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
            &Event::Stanza(ref stanza) => stanza.name == name,
            _ => false,
        }
    }

    pub fn as_stanza(&self) -> Option<&xml::Element> {
        match self {
            &Event::Stanza(ref stanza) => Some(stanza),
            _ => None,
        }
    }
}
