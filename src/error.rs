use std::io::Error as IoError;
use std::error::Error as StdError;
use std::str::Utf8Error;
use std::borrow::Cow;
use std::fmt;
use domain::resolv::error::Error as DNSError;
use domain::bits::name::FromStrError;
use native_tls::Error as TlsError;
use xmpp_parsers::error::Error as ParsersError;
use xmpp_parsers::sasl::DefinedCondition as SaslDefinedCondition;

#[derive(Debug, Error)]
pub enum Error {
    Io(IoError),
    Connection(ConnecterError),
    /// DNS label conversion error, no details available from module
    /// `idna`
    Idna,
    Domain(FromStrError),
    Protocol(ProtocolError),
    Auth(AuthError),
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

#[derive(Debug, Error)]
pub enum ProtocolError {
    Parser(ParserError),
    #[error(non_std)]
    Parsers(ParsersError),
    NoTls,
    InvalidBindResponse,
    NoStreamNamespace,
    NoStreamId,
    InvalidToken,
}

#[derive(Debug, Error)]
pub enum AuthError {
    /// No SASL mechanism available
    NoMechanism,
    #[error(no_from, non_std, msg_embedded)]
    Sasl(String),
    #[error(non_std)]
    Fail(SaslDefinedCondition),
    #[error(no_from)]
    ComponentFail,
}

#[derive(Debug, Error)]
pub enum ConnecterError {
    NoSrv,
    AllFailed,
    DNS(DNSError),
}
