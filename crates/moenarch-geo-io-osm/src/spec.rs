use std::collections::HashSet;
use std::path::PathBuf;

use geo_core::{GeoError, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
/// OpenStreetMap PBF collection specification.
pub struct OsmFilterSpec {
    /// Element and tag filtering rules.
    pub filter: OsmFilterRules,
    /// Processing settings such as node-index backend.
    pub processing: OsmProcessingSpec,
    /// Geometry output settings.
    pub output: OsmOutputSpec,
}

impl OsmFilterSpec {
    /// Parses a JSON spec, filter object, tag condition, condition list, or short tag expression.
    pub fn from_inline(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            return Err(invalid_argument("inline OSM filter must not be empty"));
        }
        if let Ok(spec) = serde_json::from_str::<Self>(input) {
            return Ok(spec);
        }
        if let Ok(filter) = serde_json::from_str::<OsmFilterRules>(input) {
            return Ok(Self {
                filter,
                ..Self::default()
            });
        }
        if let Ok(condition) = serde_json::from_str::<OsmTagCondition>(input) {
            return Ok(Self::from_include_all(vec![condition]));
        }
        if let Ok(conditions) = serde_json::from_str::<Vec<OsmTagCondition>>(input) {
            return Ok(Self::from_include_all(conditions));
        }
        if let Some(condition) = parse_tag_condition_expression(input) {
            return Ok(Self::from_include_all(vec![condition]));
        }
        Err(invalid_argument(
            "inline OSM filter must be JSON or a key=value, key!=value, key~regex, or key expression",
        ))
    }

    fn from_include_all(conditions: Vec<OsmTagCondition>) -> Self {
        Self {
            filter: OsmFilterRules {
                include: Some(OsmIncludeRules {
                    all: conditions,
                    any: Vec::new(),
                }),
                ..OsmFilterRules::default()
            },
            ..Self::default()
        }
    }

    /// Validates this spec.
    pub fn validate(&self) -> Result<()> {
        self.filter.validate()?;
        self.processing.validate()?;
        self.output.validate()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
/// OSM element and tag filter rules.
pub struct OsmFilterRules {
    /// Optional `[min_lon, min_lat, max_lon, max_lat]` bbox.
    pub bbox: Option<[f64; 4]>,
    /// Optional element type allow-list. Defaults to nodes and ways.
    pub types: Option<Vec<OsmElementType>>,
    /// Include predicates.
    pub include: Option<OsmIncludeRules>,
    /// Exclude predicates.
    pub exclude: Vec<OsmTagCondition>,
}

impl OsmFilterRules {
    /// Validates this filter.
    pub fn validate(&self) -> Result<()> {
        if let Some(bbox) = self.bbox {
            geo_core::BBox::new(bbox)?.validate_geographic()?;
        }
        let types = self
            .types
            .clone()
            .unwrap_or_else(|| vec![OsmElementType::Node, OsmElementType::Way]);
        if types.is_empty() {
            return Err(invalid_argument("filter.types must not be empty"));
        }
        if let Some(include) = &self.include {
            include.validate()?;
        }
        for condition in &self.exclude {
            condition.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
/// Include predicates for OSM tags.
pub struct OsmIncludeRules {
    /// Any of these conditions may match.
    pub any: Vec<OsmTagCondition>,
    /// All of these conditions must match.
    pub all: Vec<OsmTagCondition>,
}

impl OsmIncludeRules {
    fn validate(&self) -> Result<()> {
        for condition in self.any.iter().chain(self.all.iter()) {
            condition.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// A single OSM tag predicate.
pub struct OsmTagCondition {
    /// Tag key.
    pub key: String,
    /// Expected key presence.
    pub exists: Option<bool>,
    /// Expected exact value.
    pub value: Option<String>,
    /// Expected value set.
    pub values: Option<Vec<String>>,
    /// Expected regex pattern.
    pub regex: Option<String>,
    /// Inverts the predicate when true.
    #[serde(default, rename = "not")]
    pub negate: bool,
}

impl OsmTagCondition {
    /// Validates this condition.
    pub fn validate(&self) -> Result<()> {
        if self.key.trim().is_empty() {
            return Err(invalid_argument("condition key must not be empty"));
        }
        let operator_count = usize::from(self.exists.is_some())
            + usize::from(self.value.is_some())
            + usize::from(self.values.is_some())
            + usize::from(self.regex.is_some());
        if operator_count == 0 {
            return Err(invalid_argument(format!(
                "condition for key `{}` must include exists, value, values, or regex",
                self.key
            )));
        }
        if operator_count > 1 {
            return Err(invalid_argument(format!(
                "condition for key `{}` must use only one of exists, value, values, or regex",
                self.key
            )));
        }
        if let Some(values) = &self.values {
            if values.is_empty() {
                return Err(invalid_argument(format!(
                    "condition values for key `{}` must not be empty",
                    self.key
                )));
            }
        }
        if let Some(pattern) = &self.regex {
            Regex::new(pattern).map_err(|source| {
                invalid_argument(format!("regex `{pattern}` is invalid: {source}"))
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
/// Supported OSM element types.
pub enum OsmElementType {
    /// OSM node.
    Node,
    /// OSM way.
    Way,
    /// OSM relation.
    Relation,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
/// Processing settings.
pub struct OsmProcessingSpec {
    /// Node index settings.
    pub index: OsmIndexSpec,
}

impl OsmProcessingSpec {
    fn validate(&self) -> Result<()> {
        self.index.validate()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
/// Node index settings.
pub struct OsmIndexSpec {
    /// Index backend mode.
    pub mode: IndexMode,
    /// Number of nodes kept in memory before auto spill.
    pub memory_node_limit: usize,
    /// Optional disk index directory.
    pub disk_dir: Option<PathBuf>,
}

impl Default for OsmIndexSpec {
    fn default() -> Self {
        Self {
            mode: IndexMode::Auto,
            memory_node_limit: 5_000_000,
            disk_dir: None,
        }
    }
}

impl OsmIndexSpec {
    fn validate(&self) -> Result<()> {
        if self.memory_node_limit == 0 {
            return Err(invalid_argument(
                "processing.index.memory_node_limit must be greater than zero",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
/// Node index backend mode.
pub enum IndexMode {
    /// Start in memory and spill when disk-index is available and needed.
    #[default]
    Auto,
    /// Always keep node coordinates in memory.
    Memory,
    /// Always use disk-backed indexing.
    Disk,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
/// Output geometry settings.
pub struct OsmOutputSpec {
    /// Way geometry mode.
    pub geometry: OsmGeometryMode,
}

impl Default for OsmOutputSpec {
    fn default() -> Self {
        Self {
            geometry: OsmGeometryMode::Full,
        }
    }
}

impl OsmOutputSpec {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
/// Geometry conversion mode for OSM ways.
pub enum OsmGeometryMode {
    /// Closed ways are emitted as linestrings unless they are relations.
    #[default]
    Full,
    /// Closed ways with at least four coordinates are emitted as polygons.
    Polygon,
}

fn parse_tag_condition_expression(input: &str) -> Option<OsmTagCondition> {
    let input = input.trim();
    if input.starts_with('{') || input.starts_with('[') || input.is_empty() {
        return None;
    }
    for (operator, negate) in [("!=", true), ("=", false)] {
        if let Some((key, value)) = input.split_once(operator) {
            return Some(OsmTagCondition {
                key: parse_expression_key(key)?,
                exists: None,
                value: Some(value.trim().to_owned()),
                values: None,
                regex: None,
                negate,
            });
        }
    }
    if let Some((key, pattern)) = input.split_once('~') {
        return Some(OsmTagCondition {
            key: parse_expression_key(key)?,
            exists: None,
            value: None,
            values: None,
            regex: Some(pattern.trim().to_owned()),
            negate: false,
        });
    }
    Some(OsmTagCondition {
        key: parse_expression_key(input)?,
        exists: Some(true),
        value: None,
        values: None,
        regex: None,
        negate: false,
    })
}

fn parse_expression_key(key: &str) -> Option<String> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        None
    } else {
        Some(key.to_owned())
    }
}

pub(crate) fn validate_unique_types(types: &[OsmElementType]) -> Result<()> {
    let mut seen = HashSet::new();
    for element_type in types {
        if !seen.insert(*element_type) {
            return Err(invalid_argument("filter.types contains duplicate entries"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_spec_validates() {
        OsmFilterSpec::default().validate().unwrap();
    }

    #[test]
    fn rejects_empty_types() {
        let spec = OsmFilterSpec {
            filter: OsmFilterRules {
                types: Some(Vec::new()),
                ..OsmFilterRules::default()
            },
            ..OsmFilterSpec::default()
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn rejects_invalid_bbox() {
        let spec = OsmFilterSpec {
            filter: OsmFilterRules {
                bbox: Some([200.0, 0.0, 201.0, 1.0]),
                ..OsmFilterRules::default()
            },
            ..OsmFilterSpec::default()
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_filter_types() {
        let error = validate_unique_types(&[OsmElementType::Node, OsmElementType::Node])
            .expect_err("duplicate type");

        assert!(error.to_string().contains("duplicate"));
    }

    #[test]
    fn validates_tag_condition_operators() {
        for condition in [
            OsmTagCondition {
                key: "name".into(),
                exists: Some(true),
                value: None,
                values: None,
                regex: None,
                negate: false,
            },
            OsmTagCondition {
                key: "amenity".into(),
                exists: None,
                value: Some("school".into()),
                values: None,
                regex: None,
                negate: false,
            },
            OsmTagCondition {
                key: "amenity".into(),
                exists: None,
                value: None,
                values: Some(vec!["school".into()]),
                regex: None,
                negate: true,
            },
            OsmTagCondition {
                key: "name".into(),
                exists: None,
                value: None,
                values: None,
                regex: Some("^A".into()),
                negate: false,
            },
        ] {
            condition.validate().unwrap();
        }
    }

    #[test]
    fn parses_inline_expressions() {
        assert_eq!(
            OsmFilterSpec::from_inline("amenity=school")
                .unwrap()
                .filter
                .include
                .unwrap()
                .all[0]
                .value
                .as_deref(),
            Some("school")
        );
        assert!(
            OsmFilterSpec::from_inline("access!=private")
                .unwrap()
                .filter
                .include
                .unwrap()
                .all[0]
                .negate
        );
        assert_eq!(
            OsmFilterSpec::from_inline("name")
                .unwrap()
                .filter
                .include
                .unwrap()
                .all[0]
                .exists,
            Some(true)
        );
        assert_eq!(
            OsmFilterSpec::from_inline("name~^A")
                .unwrap()
                .filter
                .include
                .unwrap()
                .all[0]
                .regex
                .as_deref(),
            Some("^A")
        );
        assert!(OsmFilterSpec::from_inline("bad key=value").is_err());
    }
}
