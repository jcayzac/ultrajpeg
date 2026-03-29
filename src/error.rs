use thiserror::Error;

/// Result type used by the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Public error type for codec, container, and metadata failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("unsupported image format: {0}")]
    UnsupportedFormat(&'static str),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("codec error: {0}")]
    Codec(String),

    #[error("JPEG container error: {0}")]
    Container(String),

    #[error("metadata error: {0}")]
    Metadata(String),

    #[error("missing gain map image")]
    MissingGainMap,

    #[error("missing gain map metadata")]
    MissingGainMapMetadata,

    #[error("HDR reconstruction requires both a decoded gain map and parsed metadata")]
    ReconstructionUnavailable,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<img_parts::Error> for Error {
    fn from(value: img_parts::Error) -> Self {
        Self::Container(value.to_string())
    }
}

impl From<mozjpeg_rs::Error> for Error {
    fn from(value: mozjpeg_rs::Error) -> Self {
        Self::Codec(value.to_string())
    }
}

impl From<zune_jpeg::errors::DecodeErrors> for Error {
    fn from(value: zune_jpeg::errors::DecodeErrors) -> Self {
        Self::Codec(value.to_string())
    }
}

impl From<ultrahdr_core::Error> for Error {
    fn from(value: ultrahdr_core::Error) -> Self {
        Self::Metadata(value.to_string())
    }
}
