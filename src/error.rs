use derive_more::From;
use serde::Deserialize;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    #[from]
    AnyError(String),
    JsonParseError(String),
    ServiceError(String),
    NoClientError,
    InvalidAuthToken,
    CreateAuthTokenError,
    ClientReadError(String),
    ClientWriteError(String),
    ConfigReadError(String),
    ConfigParseError(String),
    ConfigInvalidError(String),
}

#[derive(Deserialize)]
pub struct ErrorResponse {
    pub status_code: u16,
    pub message: String,
    pub error: String,
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
            Self::NoClientError => write!(f, "No connected client"),
            Self::InvalidAuthToken => write!(f, "Invalid auth token"),
            Self::CreateAuthTokenError => write!(f, "Unable to create auth token"),
            Self::ClientReadError(val) => write!(f, "{}", val),
            Self::ClientWriteError(val) => write!(f, "{}", val),
            Self::ConfigReadError(val) => write!(f, "ConfigReadError: {}", val),
            Self::ConfigParseError(val) => write!(f, "ConfigParseError: {}", val),
            Self::ConfigInvalidError(val) => write!(f, "ConfigInvalidError: {}", val),
        }
    }
}
