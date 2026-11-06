//! cm1 – Ablageformat „cm1“ (Spezifikation Fassung 1).
//!
//! Gemeinsam für die Hülle (PWA), die Werkzeuge und die Tests. Das Modul braucht nur `model`
//! und `envelope` (Feature `crypto` aus). Fehlercodes wie in der Spezifikation, Abschnitt 4.

pub mod envelope;
pub mod model;

#[cfg(feature = "crypto")]
pub mod crypto;
#[cfg(feature = "crypto")]
pub mod reader;
#[cfg(feature = "crypto")]
pub mod writer;

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Hash,
    Format,
    Path,
    Sig,
    Key,
    Decrypt,
    Type,
    Old,
}

impl ErrorCode {
    pub fn code(&self) -> &'static str {
        match self {
            ErrorCode::Hash => "E_HASH",
            ErrorCode::Format => "E_FORMAT",
            ErrorCode::Path => "E_PATH",
            ErrorCode::Sig => "E_SIG",
            ErrorCode::Key => "E_KEY",
            ErrorCode::Decrypt => "E_DECRYPT",
            ErrorCode::Type => "E_TYPE",
            ErrorCode::Old => "E_OLD",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub detail: String,
}

impl Error {
    pub fn new(code: ErrorCode, detail: String) -> Self {
        Error { code, detail }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.code.code(), self.detail)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn err<T>(code: ErrorCode, detail: &str) -> Result<T> {
    Err(Error::new(code, detail.to_string()))
}
