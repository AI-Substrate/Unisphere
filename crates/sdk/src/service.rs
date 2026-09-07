//! Pure configuration policy and orchestration over core ports only.

use std::{fmt, path::Path};

use serde::{
    Deserializer,
    de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor},
};
use unisphere_core::{
    ConfigOverrides, ConfigReader, ConfigSource, Configuration, Failure, InspectionApi,
    InspectionReport, InspectionRequest, Location, MAX_CONFIG_BYTES, ReadFailure,
};

/// An inspection service with an explicitly injected reader and caller defaults.
///
/// Construction performs no I/O or validation. Each [`InspectionApi::inspect`]
/// validates every supplied layer, including layers replaced by an override.
/// The only side-effect boundary is the injected [`ConfigReader`], invoked only
/// for an explicit absolute [`ConfigSource::File`].
#[derive(Debug)]
pub struct Inspector<R: ConfigReader> {
    reader: R,
    defaults: Configuration,
}

impl<R: ConfigReader> Inspector<R> {
    /// Construct an inspector with empty source-root defaults.
    pub fn new(reader: R) -> Self {
        Self::with_defaults(reader, Configuration::default())
    }

    /// Retain caller defaults, validating them on each inspection.
    pub fn with_defaults(reader: R, defaults: Configuration) -> Self {
        Self { reader, defaults }
    }
}

impl<R: ConfigReader> InspectionApi for Inspector<R> {
    fn inspect(&self, request: &InspectionRequest) -> Result<InspectionReport, Failure> {
        validate_roots(&self.defaults.source_roots)?;
        let document = match &request.source {
            ConfigSource::Defaults => None,
            ConfigSource::Inline(bytes) => {
                if bytes.len() > MAX_CONFIG_BYTES {
                    return Err(Failure::invalid_configuration(None));
                }
                parse_document(bytes, None)?
            }
            ConfigSource::File(path) => {
                if !path.is_absolute() {
                    return Err(Failure::invalid_configuration(Some(file_location(path))));
                }
                let bytes = self
                    .reader
                    .read(path, MAX_CONFIG_BYTES)
                    .map_err(|kind| Failure::configuration_read(kind, Some(file_location(path))))?;
                // Defend the service boundary even if an injected reader violates
                // its limit. Do not parse or copy that oversized result.
                if bytes.len() > MAX_CONFIG_BYTES {
                    return Err(Failure::configuration_read(
                        ReadFailure::TooLarge,
                        Some(file_location(path)),
                    ));
                }
                parse_document(&bytes, Some(path))?
            }
        };
        Ok(InspectionReport {
            configuration: resolve(&self.defaults, document, &request.overrides)?,
        })
    }
}

// Defaults and document have already been validated. Only the selected borrowed
// layer is cloned; a selected parsed document is moved into the report.
fn resolve(
    defaults: &Configuration,
    document: Option<Vec<String>>,
    overrides: &ConfigOverrides,
) -> Result<Configuration, Failure> {
    let source_roots = if let Some(roots) = &overrides.source_roots {
        validate_roots(roots)?;
        roots.clone()
    } else {
        document.unwrap_or_else(|| defaults.source_roots.clone())
    };
    Ok(Configuration { source_roots })
}

fn validate_roots(roots: &[String]) -> Result<(), Failure> {
    if let Some(index) = roots.iter().position(|root| root.trim().is_empty()) {
        return Err(Failure::invalid_configuration(Some(Location {
            field: Some(format!("source_roots[{index}]")),
            ..Location::default()
        })));
    }
    Ok(())
}

fn file_location(path: &Path) -> Location {
    Location {
        path: Some(path.to_string_lossy().into_owned()),
        ..Location::default()
    }
}

fn parse_document(bytes: &[u8], path: Option<&Path>) -> Result<Option<Vec<String>>, Failure> {
    let mut location = ParseField::Document;
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    let document = parser
        .deserialize_map(DocumentVisitor {
            location: &mut location,
        })
        .and_then(|document| {
            parser.end()?;
            Ok(document)
        });
    document.map_err(|error| {
        // Only numeric coordinates survive the parser boundary. Its prose can
        // contain configuration keys/values and must never enter public errors.
        Failure::invalid_configuration(Some(Location {
            path: path.map(|path| path.to_string_lossy().into_owned()),
            field: match location {
                ParseField::Document => None,
                ParseField::Roots => Some("source_roots".into()),
                ParseField::Root(index) => Some(format!("source_roots[{index}]")),
            },
            line: u32::try_from(error.line()).ok(),
            column: u32::try_from(error.column()).ok(),
        }))
    })
}

// Structural context only; successful parsing allocates no diagnostic strings.
enum ParseField {
    Document,
    Roots,
    Root(usize),
}

struct DocumentVisitor<'a> {
    location: &'a mut ParseField,
}

impl<'de> Visitor<'de> for DocumentVisitor<'_> {
    type Value = Option<Vec<String>>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a configuration object")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut roots = None;
        loop {
            *self.location = ParseField::Document;
            let Some(key) = map.next_key::<String>()? else {
                return Ok(roots);
            };
            if key != "source_roots" {
                return Err(M::Error::custom("unknown configuration field"));
            }
            *self.location = ParseField::Roots;
            if roots.is_some() {
                return Err(M::Error::custom("duplicate source_roots field"));
            }
            // Do not deserialize Option: present null is invalid, not omission.
            roots = Some(map.next_value_seed(RootsVisitor {
                location: &mut *self.location,
            })?);
        }
    }
}

struct RootsVisitor<'a> {
    location: &'a mut ParseField,
}

impl<'de> DeserializeSeed<'de> for RootsVisitor<'_> {
    type Value = Vec<String>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for RootsVisitor<'_> {
    type Value = Vec<String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of non-blank strings")
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Self::Value, S::Error> {
        let mut roots = Vec::new();
        loop {
            *self.location = ParseField::Root(roots.len());
            let Some(root) = sequence.next_element::<String>()? else {
                *self.location = ParseField::Roots;
                return Ok(roots);
            };
            if root.trim().is_empty() {
                return Err(S::Error::custom("blank source root"));
            }
            roots.push(root);
        }
    }
}
