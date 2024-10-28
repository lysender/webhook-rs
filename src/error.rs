use derive_more::From;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    #[from]
    AnyError(String),
    JsonParseError(String),
    ServiceError(String),
    WebSocketClosed,
    NoClientError,
    InvalidAuthToken,
    CreateAuthTokenError,
    ClientReadError(String),
    ClientWriteError(String),
    ConfigReadError(String),
    ConfigParseError(String),
    ConfigInvalidError(String),
    TunnelMessageInvalid,
    TunnelStatusLineInvalid,
    RequestLineInvalid(String),
    ResponseLineInvalid(String),
    RequestHeaderTooLarge,
    RequestHeaderInvalid,
    ResponseHeaderTooLarge,
    ResponseHeaderInvalid,
}

/// Allow string slices to be converted to Error
impl From<&str> for Error {
    fn from(val: &str) -> Self {
        Self::AnyError(val.to_string())
    }
}

/// Allow errors to be displayed as string
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::AnyError(val) => write!(f, "{}", val),
            Self::JsonParseError(val) => write!(f, "{}", val),
            Self::ServiceError(val) => write!(f, "{}", val),
            Self::WebSocketClosed => write!(f, "WebSocket connection closed"),
            Self::NoClientError => write!(f, "No connected client"),
            Self::InvalidAuthToken => write!(f, "Invalid auth token"),
            Self::CreateAuthTokenError => write!(f, "Unable to create auth token"),
            Self::ClientReadError(val) => write!(f, "{}", val),
            Self::ClientWriteError(val) => write!(f, "{}", val),
            Self::ConfigReadError(val) => write!(f, "ConfigReadError: {}", val),
            Self::ConfigParseError(val) => write!(f, "ConfigParseError: {}", val),
            Self::ConfigInvalidError(val) => write!(f, "ConfigInvalidError: {}", val),
            Self::TunnelMessageInvalid => write!(f, "Tunnel message malformed"),
            Self::TunnelStatusLineInvalid => write!(f, "Tunnel status line malformed"),
            Self::RequestLineInvalid(val) => write!(f, "RequestLineInvalid: {}", val),
            Self::ResponseLineInvalid(val) => write!(f, "ResponseLineInvalid: {}", val),
            Self::RequestHeaderTooLarge => write!(f, "Request header too large"),
            Self::RequestHeaderInvalid => write!(f, "Request header malformed"),
            Self::ResponseHeaderTooLarge => write!(f, "Response header too large"),
            Self::ResponseHeaderInvalid => write!(f, "Response header malformed"),
        }
    }
}
