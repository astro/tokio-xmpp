use std::io::Error as IoError;
use std::error::Error as StdError;
use std::str::Utf8Error;
use std::borrow::Cow;
use std::fmt;
use native_tls::Error as TlsError;
use trust_dns_resolver::error::ResolveError;
use trust_dns_proto::error::ProtoError;

use xmpp_parsers::error::Error as ParsersError;
use xmpp_parsers::sasl::DefinedCondition as SaslDefinedCondition;

/// Top-level error type
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error
    Io(IoError),
    /// Error resolving DNS and establishing a connection
    Connection(ConnecterError),
    /// DNS label conversion error, no details available from module
    /// `idna`
    Idna,
    /// Protocol-level error
    Protocol(ProtocolError),
    /// Authentication error
    Auth(AuthError),
    /// TLS error
    Tls(TlsError),
    /// Shoud never happen
    InvalidState,
}

/// Causes for stream parsing errors
#[derive(Debug, Error)]
pub enum ParserError {
    /// Encoding error
    Utf8(Utf8Error),
    /// XML parse error
    Parse(ParseError),
    /// Illegal `</>`
    ShortTag,
    /// Required by `impl Decoder`
    IO(IoError),
}

impl From<ParserError> for Error {
    fn from(e: ParserError) -> Self {
        ProtocolError::Parser(e).into()
    }
}

/// XML parse error wrapper type
#[derive(Debug)]
pub struct ParseError(pub Cow<'static, str>);

impl StdError for ParseError {
    fn description(&self) -> &str {
        self.0.as_ref()
    }
    fn cause(&self) -> Option<&StdError> {
        None
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// XMPP protocol-level error
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// XML parser error
    Parser(ParserError),
    /// Error with expected stanza schema
    #[error(non_std)]
    Parsers(ParsersError),
    /// No TLS available
    NoTls,
    /// Invalid response to resource binding
    InvalidBindResponse,
    /// No xmlns attribute in <stream:stream>
    NoStreamNamespace,
    /// No id attribute in <stream:stream>
    NoStreamId,
    /// Encountered an unexpected XML token
    InvalidToken,
}

/// Authentication error
#[derive(Debug, Error)]
pub enum AuthError {
    /// No matching SASL mechanism available
    NoMechanism,
    /// Local SASL implementation error
    #[error(no_from, non_std, msg_embedded)]
    Sasl(String),
    /// Failure from server
    #[error(non_std)]
    Fail(SaslDefinedCondition),
    /// Component authentication failure
    #[error(no_from)]
    ComponentFail,
}

/// Error establishing connection
#[derive(Debug, Error)]
pub enum ConnecterError {
    /// All attempts failed, no error available
    AllFailed,
    /// DNS protocol error
    Dns(ProtoError),
    /// DNS resolution error
    Resolve(ResolveError),
}
