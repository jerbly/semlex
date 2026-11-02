//! Serde model of Weaver's `resolved/2.0` registry format and the effective
//! view that the rest of semlex consumes.
//!
//! ```no_run
//! let registry = semlex_schema::load("resolved.yaml")?;
//! for group in &registry.groups {
//!     println!("{} {}: {} attributes", group.kind.tag().noun(), group.id, group.attributes.len());
//! }
//! # Ok::<(), semlex_schema::Error>(())
//! ```

pub mod annotations;
pub mod model;
pub mod names;
pub mod view;

pub use annotations::{AnnotationError, Annotations};
pub use view::{
    AttributeDef, EffectiveAttribute, EnumDef, EnumMember, Group, GroupKind, GroupKindTag,
    Primitive, Refines, Registry, Requirement, Role, ValueType,
};

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a registry couldn't be loaded.
#[derive(Debug)]
pub enum Error {
    /// The file couldn't be read.
    Io {
        /// The path that was read.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// `file_format` is missing or isn't `resolved/2.0`.
    UnsupportedFormat {
        /// The value found, if any.
        found: Option<String>,
    },
    /// The YAML is malformed or doesn't match the format.
    Parse(String),
    /// The file parses but is inconsistent (dangling index, mixed enum types, …).
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => {
                write!(f, "can't read registry `{}`: {source}", path.display())
            }
            Error::UnsupportedFormat { found: Some(found) } => write!(
                f,
                "unsupported registry file_format `{found}`; semlex reads `{}` (produce it with `weaver registry package --v2`)",
                model::FILE_FORMAT
            ),
            Error::UnsupportedFormat { found: None } => write!(
                f,
                "not a resolved registry: `file_format` is missing; semlex reads `{}` (produce it with `weaver registry package --v2`)",
                model::FILE_FORMAT
            ),
            Error::Parse(msg) => write!(f, "can't parse registry: {msg}"),
            Error::Invalid(msg) => write!(f, "invalid registry: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Reads and parses a `resolved.yaml`.
pub fn load(path: impl AsRef<Path>) -> Result<Registry, Error> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&text)
}

/// Parses the text of a `resolved.yaml`.
pub fn parse(text: &str) -> Result<Registry, Error> {
    view::build(parse_model(text)?)
}

/// Parses into the raw [`model`] without building the view.
pub fn parse_model(text: &str) -> Result<model::Resolved, Error> {
    // Check the format first, so a different version gets a clear message
    // instead of whatever field mismatch the full parse hits first.
    #[derive(serde::Deserialize)]
    struct Probe {
        file_format: Option<String>,
    }
    let probe: Probe = from_yaml(text)?;
    if probe.file_format.as_deref() != Some(model::FILE_FORMAT) {
        return Err(Error::UnsupportedFormat {
            found: probe.file_format,
        });
    }
    from_yaml(text)
}

/// YAML as Weaver writes it: the 1.2 core schema, where only `true` and
/// `false` are booleans. Weaver leaves `on`, `off`, `yes`, `no` (any case)
/// unquoted as strings, which YAML 1.1 would read as booleans: an enum
/// member `id: on` with `value: ON` must stay a string.
fn from_yaml<'de, T: serde::Deserialize<'de>>(text: &'de str) -> Result<T, Error> {
    let options = serde_saphyr::options! { strict_booleans: true };
    serde_saphyr::from_str_with_options(text, options).map_err(|e| Error::Parse(e.to_string()))
}
