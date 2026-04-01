use thiserror::Error;

/// Result type used by the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Public error type for codec, container, and metadata failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The caller requested an unsupported pixel format, JPEG coding mode, or
    /// reconstruction output.
    #[error("unsupported image format: {0}")]
    UnsupportedFormat(&'static str),

    /// The caller provided invalid input, such as mismatched image dimensions,
    /// missing required metadata, or an invalid option combination.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// JPEG encoding or decoding failed.
    #[error("codec error: {0}")]
    Codec(String),

    /// JPEG marker parsing, MPF processing, or container assembly failed.
    #[error("JPEG container error: {0}")]
    Container(String),

    /// Ultra HDR metadata parsing, validation, or synthesis failed.
    #[error("metadata error: {0}")]
    Metadata(String),

    /// HDR reconstruction was requested on a decoded image that did not include
    /// a gain map.
    #[error("missing gain map image")]
    MissingGainMap,

    /// HDR reconstruction was requested on a decoded image whose gain-map
    /// metadata could not be resolved.
    #[error("missing gain map metadata")]
    MissingGainMapMetadata,

    /// HDR reconstruction prerequisites were not satisfied.
    #[error("HDR reconstruction requires both a decoded gain map and parsed metadata")]
    ReconstructionUnavailable,

    /// I/O failed while reading or writing external data.
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
