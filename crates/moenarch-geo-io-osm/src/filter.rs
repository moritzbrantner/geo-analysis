use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use geo_core::{
    assemble_multipolygon, BBox, Coordinate, GeoError, GeoFeatureCollection, Geometry, Result,
};
use osmpbfreader::{
    NodeId, OsmObj, OsmPbfReader, Relation, RelationId, Tags as OsmPbfTags, Way, WayId,
};
use regex::Regex;
use serde::Serialize;

use crate::index::{AutoNodeIndex, IndexBackend, IndexOptions, NodeIndex, StoredCoordinate};
use crate::model::{geo_collection_from_osm, OsmElementKind, OsmFeature, OsmTags};
use crate::spec::{
    validate_unique_types, OsmElementType, OsmFilterSpec, OsmGeometryMode, OsmIncludeRules,
    OsmTagCondition,
};

const NODE_INDEX_BATCH_SIZE: usize = 16_384;

fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}

fn source_error(message: impl Into<String>) -> GeoError {
    GeoError::source(message)
}

#[derive(Debug, Clone)]
/// Options for collecting OSM features from a PBF file.
pub struct CollectOsmOptions {
    /// Input `.osm.pbf` file.
    pub input: PathBuf,
    /// Filter spec.
    pub spec: OsmFilterSpec,
    /// Node index options.
    pub index_options: IndexOptions,
}

#[derive(Debug, Clone)]
/// Options for collecting OSM features from PBF bytes.
pub struct CollectOsmBytesOptions<'a> {
    /// Input PBF bytes.
    pub input: &'a [u8],
    /// Filter spec.
    pub spec: OsmFilterSpec,
    /// Node index options.
    pub index_options: IndexOptions,
}

#[derive(Debug, Clone)]
/// Collected OSM features plus run report.
pub struct OsmFeatureCollection {
    /// Matching OSM features.
    pub features: Vec<OsmFeature>,
    /// Collection report.
    pub report: CollectOsmReport,
}

impl OsmFeatureCollection {
    /// Converts this collection into a `geo-core` feature collection.
    pub fn into_geo_feature_collection(self) -> GeoFeatureCollection {
        geo_collection_from_osm(self.features)
    }

    /// Converts this collection into a `geo-core` feature collection without consuming it.
    pub fn to_geo_feature_collection(&self) -> GeoFeatureCollection {
        geo_collection_from_osm(self.features.clone())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// OSM collection report.
pub struct CollectOsmReport {
    /// Number of emitted objects.
    pub objects_collected: u64,
    /// Ways skipped because node references could not be resolved.
    pub ways_skipped_missing_nodes: u64,
    /// Relations skipped because they are not supported area relations.
    pub relations_skipped_non_area: u64,
    /// Relations skipped because required member ways were missing.
    pub relations_skipped_missing_members: u64,
    /// Relations skipped because rings could not be assembled.
    pub relations_skipped_invalid_rings: u64,
    /// Relation members ignored because kind or role is unsupported.
    pub relation_members_ignored_role: u64,
    /// Actual node index backend used.
    pub index_backend: IndexBackend,
}

impl Default for CollectOsmReport {
    fn default() -> Self {
        Self {
            objects_collected: 0,
            ways_skipped_missing_nodes: 0,
            relations_skipped_non_area: 0,
            relations_skipped_missing_members: 0,
            relations_skipped_invalid_rings: 0,
            relation_members_ignored_role: 0,
            index_backend: IndexBackend::Memory,
        }
    }
}

/// Collects OSM features from a PBF file.
pub fn collect_osm_pbf(options: CollectOsmOptions) -> Result<OsmFeatureCollection> {
    validate_input_path(&options.input)?;
    let input = options.input.clone();
    run_filter_pipeline(
        input.clone(),
        || File::open(&input).map_err(GeoError::Io),
        &options.spec,
        options.index_options,
    )
}

/// Collects OSM features from in-memory PBF bytes.
pub fn collect_osm_pbf_bytes(options: CollectOsmBytesOptions<'_>) -> Result<OsmFeatureCollection> {
    run_filter_pipeline(
        PathBuf::from("<memory>"),
        || Ok(Cursor::new(options.input)),
        &options.spec,
        options.index_options,
    )
}

fn run_filter_pipeline<R, OpenReader>(
    input: PathBuf,
    mut open_reader: OpenReader,
    spec: &OsmFilterSpec,
    index_options: IndexOptions,
) -> Result<OsmFeatureCollection>
where
    R: Read,
    OpenReader: FnMut() -> Result<R>,
{
    let compiled = CompiledOsmFilter::compile(spec)?;
    let includes_way = compiled.includes_type(OsmElementType::Way);
    let includes_relation = compiled.includes_type(OsmElementType::Relation);
    let needs_node_index = includes_way || includes_relation;
    let mut node_index = if needs_node_index {
        Some(AutoNodeIndex::create(index_options)?)
    } else {
        None
    };
    let mut sink = VecFeatureSink::new();
    let mut report = CollectOsmReport::default();
    let mut candidates = Vec::new();
    let mut required_way_ids = HashSet::new();

    process_first_pass_from_reader(
        &input,
        open_reader()?,
        FirstPassContext {
            compiled: &compiled,
            node_index: node_index.as_mut().map(|index| index as &mut dyn NodeIndex),
            sink: &mut sink,
            candidates: &mut candidates,
            required_way_ids: &mut required_way_ids,
            report: &mut report,
        },
    )?;

    let mut relation_way_geometries = HashMap::new();
    if needs_node_index && (includes_way || !required_way_ids.is_empty()) {
        process_ways_from_reader(
            &input,
            open_reader()?,
            WayPassContext {
                compiled: &compiled,
                node_index: node_index
                    .as_ref()
                    .expect("node index exists when ways are processed"),
                required_way_ids: &required_way_ids,
                relation_way_geometries: &mut relation_way_geometries,
                sink: &mut sink,
                report: &mut report,
            },
        )?;
    }

    if includes_relation {
        emit_relations(
            &compiled,
            &candidates,
            &relation_way_geometries,
            &mut sink,
            &mut report,
        )?;
    }

    if let Some(node_index) = &node_index {
        report.index_backend = node_index.backend();
    }

    Ok(OsmFeatureCollection {
        features: sink.features,
        report,
    })
}

fn process_first_pass_from_reader<R: Read>(
    input: &Path,
    reader: R,
    mut context: FirstPassContext<'_>,
) -> Result<()> {
    let mut reader = OsmPbfReader::new(reader);
    let mut node_batch = Vec::with_capacity(NODE_INDEX_BATCH_SIZE);
    let mut node_features = Vec::new();
    let should_index_nodes = context.node_index.is_some();

    for object in reader.iter() {
        let object = object.map_err(|source| {
            source_error(format!(
                "OSM PBF parsing failed for `{}`: {source}",
                input.display()
            ))
        })?;
        match object {
            OsmObj::Node(node) => {
                process_node(
                    node.id,
                    StoredCoordinate::new(node.decimicro_lon, node.decimicro_lat),
                    &node.tags,
                    NodeProcessingContext {
                        compiled: context.compiled,
                        should_index_node: should_index_nodes,
                        node_batch: &mut node_batch,
                        node_features: &mut node_features,
                        report: context.report,
                    },
                )?;
                if node_batch.len() >= NODE_INDEX_BATCH_SIZE {
                    if let Some(index) = context.node_index.as_mut() {
                        flush_indexed_node_batch(
                            &mut **index,
                            &mut node_batch,
                            &mut node_features,
                            context.sink,
                        )?;
                    }
                } else if !should_index_nodes {
                    flush_node_features(&mut node_features, context.sink)?;
                }
            }
            OsmObj::Relation(relation)
                if context.compiled.includes_type(OsmElementType::Relation) =>
            {
                collect_relation(
                    relation,
                    context.compiled,
                    context.candidates,
                    context.required_way_ids,
                    context.report,
                );
            }
            _ => {}
        }
    }

    if let Some(index) = context.node_index.as_mut() {
        flush_indexed_node_batch(
            &mut **index,
            &mut node_batch,
            &mut node_features,
            context.sink,
        )?;
    }
    flush_node_features(&mut node_features, context.sink)
}

struct FirstPassContext<'a> {
    compiled: &'a CompiledOsmFilter,
    node_index: Option<&'a mut dyn NodeIndex>,
    sink: &'a mut dyn FeatureSink,
    candidates: &'a mut Vec<CandidateRelation>,
    required_way_ids: &'a mut HashSet<WayId>,
    report: &'a mut CollectOsmReport,
}

fn flush_indexed_node_batch(
    node_index: &mut dyn NodeIndex,
    node_batch: &mut Vec<(NodeId, StoredCoordinate)>,
    node_features: &mut Vec<OsmFeature>,
    sink: &mut dyn FeatureSink,
) -> Result<()> {
    if !node_batch.is_empty() {
        node_index.insert_batch(node_batch)?;
        node_batch.clear();
    }
    flush_node_features(node_features, sink)
}

fn flush_node_features(
    node_features: &mut Vec<OsmFeature>,
    sink: &mut dyn FeatureSink,
) -> Result<()> {
    for feature in node_features.drain(..) {
        sink.write_feature(feature)?;
    }
    Ok(())
}

#[cfg(test)]
fn process_node_for_test(
    node_id: NodeId,
    coordinate: StoredCoordinate,
    tags: &OsmPbfTags,
    compiled: &CompiledOsmFilter,
    node_index: &mut dyn NodeIndex,
    sink: &mut dyn FeatureSink,
    report: &mut CollectOsmReport,
) -> Result<()> {
    let mut node_batch = Vec::with_capacity(1);
    let mut node_features = Vec::new();
    process_node(
        node_id,
        coordinate,
        tags,
        NodeProcessingContext {
            compiled,
            should_index_node: true,
            node_batch: &mut node_batch,
            node_features: &mut node_features,
            report,
        },
    )?;
    node_index.insert_batch(&node_batch)?;
    flush_node_features(&mut node_features, sink)
}

struct NodeProcessingContext<'a> {
    compiled: &'a CompiledOsmFilter,
    should_index_node: bool,
    node_batch: &'a mut Vec<(NodeId, StoredCoordinate)>,
    node_features: &'a mut Vec<OsmFeature>,
    report: &'a mut CollectOsmReport,
}

fn process_node(
    node_id: NodeId,
    coordinate: StoredCoordinate,
    tags: &OsmPbfTags,
    context: NodeProcessingContext<'_>,
) -> Result<()> {
    if context.should_index_node {
        context.node_batch.push((node_id, coordinate));
    }

    if !context.compiled.includes_type(OsmElementType::Node) {
        return Ok(());
    }

    let coordinate = coordinate.to_coordinate()?;
    if !context.compiled.matches_node_bbox(coordinate) {
        return Ok(());
    }

    let tags = normalize_tags(tags);
    if context.compiled.matches_node_tags(&tags) {
        context.node_features.push(OsmFeature {
            id: node_id.0,
            kind: OsmElementKind::Node,
            tags,
            geometry: geo_core::point(coordinate),
        });
        context.report.objects_collected += 1;
    }
    Ok(())
}

fn process_ways_from_reader<R: Read>(
    input: &Path,
    reader: R,
    context: WayPassContext<'_>,
) -> Result<()> {
    let mut reader = OsmPbfReader::new(reader);
    for object in reader.iter() {
        let object = object.map_err(|source| {
            source_error(format!(
                "OSM PBF parsing failed for `{}`: {source}",
                input.display()
            ))
        })?;
        if let OsmObj::Way(way) = object {
            process_way(
                &way,
                context.compiled,
                context.node_index,
                context.required_way_ids,
                context.relation_way_geometries,
                context.sink,
                context.report,
            )?;
        }
    }
    Ok(())
}

struct WayPassContext<'a> {
    compiled: &'a CompiledOsmFilter,
    node_index: &'a dyn NodeIndex,
    required_way_ids: &'a HashSet<WayId>,
    relation_way_geometries: &'a mut HashMap<WayId, Vec<Coordinate>>,
    sink: &'a mut dyn FeatureSink,
    report: &'a mut CollectOsmReport,
}

fn collect_relation(
    relation: Relation,
    compiled: &CompiledOsmFilter,
    candidates: &mut Vec<CandidateRelation>,
    required_way_ids: &mut HashSet<WayId>,
    report: &mut CollectOsmReport,
) {
    let tags = normalize_tags(&relation.tags);
    if !compiled.matches_relation_tags(&tags) {
        return;
    }

    if !is_area_relation(&tags) {
        report.relations_skipped_non_area += 1;
        return;
    }

    let mut members = Vec::new();
    for member in relation.refs {
        let role = member.role.as_str();
        let Some(way_id) = member.member.way() else {
            report.relation_members_ignored_role += 1;
            continue;
        };
        let member_role = match role {
            "" | "outer" => RelationMemberRole::Outer,
            "inner" => RelationMemberRole::Inner,
            _ => {
                report.relation_members_ignored_role += 1;
                continue;
            }
        };
        required_way_ids.insert(way_id);
        members.push(RelationWayMember {
            way_id,
            role: member_role,
        });
    }

    candidates.push(CandidateRelation {
        id: relation.id,
        tags,
        members,
    });
}

fn process_way(
    way: &Way,
    compiled: &CompiledOsmFilter,
    node_index: &dyn NodeIndex,
    required_way_ids: &HashSet<WayId>,
    relation_way_geometries: &mut HashMap<WayId, Vec<Coordinate>>,
    sink: &mut dyn FeatureSink,
    report: &mut CollectOsmReport,
) -> Result<()> {
    let mut coordinates = match coordinates_for_way(&way.nodes, node_index)? {
        Some(coordinates) => coordinates,
        None => {
            if compiled.includes_type(OsmElementType::Way) {
                report.ways_skipped_missing_nodes += 1;
            }
            return Ok(());
        }
    };

    if compiled.includes_type(OsmElementType::Way) {
        let tags = normalize_tags(&way.tags);
        if compiled.matches_way(&tags, &coordinates) {
            sink.write_feature(OsmFeature {
                id: way.id.0,
                kind: OsmElementKind::Way,
                tags,
                geometry: way_geometry(&coordinates, way.is_closed(), compiled.geometry_mode),
            })?;
            report.objects_collected += 1;
        }
    }

    if required_way_ids.contains(&way.id) {
        relation_way_geometries.insert(way.id, std::mem::take(&mut coordinates));
    }

    Ok(())
}

fn emit_relations(
    compiled: &CompiledOsmFilter,
    candidates: &[CandidateRelation],
    relation_way_geometries: &HashMap<WayId, Vec<Coordinate>>,
    sink: &mut dyn FeatureSink,
    report: &mut CollectOsmReport,
) -> Result<()> {
    for candidate in candidates {
        let geometry = match assemble_relation(candidate, relation_way_geometries) {
            RelationAssemblyResult::Geometry(geometry) => geometry,
            RelationAssemblyResult::MissingMembers => {
                report.relations_skipped_missing_members += 1;
                continue;
            }
            RelationAssemblyResult::InvalidRings => {
                report.relations_skipped_invalid_rings += 1;
                continue;
            }
        };

        if !compiled.matches_relation_geometry(&geometry) {
            continue;
        }

        sink.write_feature(OsmFeature {
            id: candidate.id.0,
            kind: OsmElementKind::Relation,
            tags: candidate.tags.clone(),
            geometry,
        })?;
        report.objects_collected += 1;
    }
    Ok(())
}

fn coordinates_for_way(
    nodes: &[NodeId],
    node_index: &dyn NodeIndex,
) -> Result<Option<Vec<Coordinate>>> {
    let mut coordinates = Vec::with_capacity(nodes.len());
    for node_id in nodes {
        let Some(coordinate) = node_index.get(*node_id)? else {
            return Ok(None);
        };
        coordinates.push(coordinate.to_coordinate()?);
    }
    Ok(Some(coordinates))
}

fn normalize_tags(tags: &OsmPbfTags) -> OsmTags {
    tags.iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn validate_input_path(path: &Path) -> Result<()> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if filename.ends_with(".osm.pbf") || filename.ends_with(".pbf") {
        Ok(())
    } else {
        Err(invalid_argument(format!(
            "unsupported OSM input file `{}`: expected an .osm.pbf file",
            path.display()
        )))
    }
}

trait FeatureSink {
    fn write_feature(&mut self, feature: OsmFeature) -> Result<()>;
}

#[derive(Debug, Clone)]
struct VecFeatureSink {
    features: Vec<OsmFeature>,
}

impl VecFeatureSink {
    fn new() -> Self {
        Self {
            features: Vec::new(),
        }
    }
}

impl FeatureSink for VecFeatureSink {
    fn write_feature(&mut self, feature: OsmFeature) -> Result<()> {
        self.features.push(feature);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct CandidateRelation {
    id: RelationId,
    tags: OsmTags,
    members: Vec<RelationWayMember>,
}

#[derive(Debug, Clone, Copy)]
struct RelationWayMember {
    way_id: WayId,
    role: RelationMemberRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationMemberRole {
    Outer,
    Inner,
}

#[derive(Debug)]
enum RelationAssemblyResult {
    Geometry(Geometry),
    MissingMembers,
    InvalidRings,
}

fn is_area_relation(tags: &OsmTags) -> bool {
    matches!(
        tags.get("type").map(String::as_str),
        Some("multipolygon" | "boundary")
    )
}

fn assemble_relation(
    candidate: &CandidateRelation,
    relation_way_geometries: &HashMap<WayId, Vec<Coordinate>>,
) -> RelationAssemblyResult {
    let mut outer_segments = Vec::new();
    let mut inner_segments = Vec::new();
    for member in &candidate.members {
        let Some(coordinates) = relation_way_geometries.get(&member.way_id) else {
            return RelationAssemblyResult::MissingMembers;
        };
        match member.role {
            RelationMemberRole::Outer => outer_segments.push(coordinates.clone()),
            RelationMemberRole::Inner => inner_segments.push(coordinates.clone()),
        }
    }

    match assemble_multipolygon(outer_segments, inner_segments) {
        Ok(geometry) => RelationAssemblyResult::Geometry(geometry),
        Err(_) => RelationAssemblyResult::InvalidRings,
    }
}

fn way_geometry(
    coordinates: &[Coordinate],
    is_closed: bool,
    geometry_mode: OsmGeometryMode,
) -> Geometry {
    let line = coordinates
        .iter()
        .copied()
        .map(Coordinate::as_position)
        .collect::<Vec<_>>();

    if geometry_mode == OsmGeometryMode::Polygon && is_closed && line.len() >= 4 {
        Geometry::Polygon {
            coordinates: vec![line],
        }
    } else {
        Geometry::LineString { coordinates: line }
    }
}

#[derive(Debug)]
/// Compiled OSM filter predicates.
pub struct CompiledOsmFilter {
    types: HashSet<OsmElementType>,
    bbox: Option<BBox>,
    include_any: Vec<CompiledCondition>,
    include_all: Vec<CompiledCondition>,
    exclude: Vec<CompiledCondition>,
    /// Way geometry mode.
    pub geometry_mode: OsmGeometryMode,
}

impl CompiledOsmFilter {
    /// Compiles a filter spec.
    pub fn compile(spec: &OsmFilterSpec) -> Result<Self> {
        spec.validate()?;
        let raw_types = spec
            .filter
            .types
            .clone()
            .unwrap_or_else(|| vec![OsmElementType::Node, OsmElementType::Way]);
        validate_unique_types(&raw_types)?;
        let types = raw_types.into_iter().collect();
        let OsmIncludeRules { any, all } = spec.filter.include.clone().unwrap_or_default();
        let bbox = spec.filter.bbox.map(BBox::new).transpose()?;

        Ok(Self {
            types,
            bbox,
            include_any: compile_conditions(&any)?,
            include_all: compile_conditions(&all)?,
            exclude: compile_conditions(&spec.filter.exclude)?,
            geometry_mode: spec.output.geometry,
        })
    }

    /// Returns true when this filter includes an element type.
    pub fn includes_type(&self, element_type: OsmElementType) -> bool {
        self.types.contains(&element_type)
    }

    /// Returns true when a node matches all filters.
    pub fn matches_node(&self, tags: &OsmTags, coordinate: Coordinate) -> bool {
        self.types.contains(&OsmElementType::Node)
            && self.matches_node_tags(tags)
            && self.matches_node_bbox(coordinate)
    }

    fn matches_node_tags(&self, tags: &OsmTags) -> bool {
        self.types.contains(&OsmElementType::Node) && self.matches_tags(tags)
    }

    fn matches_node_bbox(&self, coordinate: Coordinate) -> bool {
        self.bbox
            .map(|bbox| bbox.contains(coordinate))
            .unwrap_or(true)
    }

    /// Returns true when a way matches all filters.
    pub fn matches_way(&self, tags: &OsmTags, coordinates: &[Coordinate]) -> bool {
        self.types.contains(&OsmElementType::Way)
            && self.matches_tags(tags)
            && self
                .bbox
                .map(|bbox| {
                    coordinates
                        .iter()
                        .copied()
                        .any(|coordinate| bbox.contains(coordinate))
                })
                .unwrap_or(true)
    }

    fn matches_relation_tags(&self, tags: &OsmTags) -> bool {
        self.types.contains(&OsmElementType::Relation) && self.matches_tags(tags)
    }

    fn matches_relation_geometry(&self, geometry: &Geometry) -> bool {
        self.types.contains(&OsmElementType::Relation)
            && self
                .bbox
                .map(|bbox| bbox.intersects_geometry(geometry))
                .unwrap_or(true)
    }

    fn matches_tags(&self, tags: &OsmTags) -> bool {
        if self.exclude.iter().any(|condition| condition.matches(tags)) {
            return false;
        }
        if !self.include_any.is_empty()
            && !self
                .include_any
                .iter()
                .any(|condition| condition.matches(tags))
        {
            return false;
        }
        self.include_all
            .iter()
            .all(|condition| condition.matches(tags))
    }
}

fn compile_conditions(conditions: &[OsmTagCondition]) -> Result<Vec<CompiledCondition>> {
    conditions.iter().map(CompiledCondition::compile).collect()
}

#[derive(Debug)]
/// Compiled tag condition.
pub struct CompiledCondition {
    key: String,
    operator: ConditionOperator,
    negate: bool,
}

impl CompiledCondition {
    /// Compiles a tag condition.
    pub fn compile(condition: &OsmTagCondition) -> Result<Self> {
        condition.validate()?;
        let operator = if let Some(exists) = condition.exists {
            ConditionOperator::Exists(exists)
        } else if let Some(value) = &condition.value {
            ConditionOperator::Value(value.clone())
        } else if let Some(values) = &condition.values {
            ConditionOperator::Values(values.iter().cloned().collect())
        } else if let Some(pattern) = &condition.regex {
            ConditionOperator::Regex(Regex::new(pattern).map_err(|source| {
                invalid_argument(format!("regex `{pattern}` is invalid: {source}"))
            })?)
        } else {
            return Err(invalid_argument(format!(
                "condition for key `{}` is missing an operator",
                condition.key
            )));
        };
        Ok(Self {
            key: condition.key.clone(),
            operator,
            negate: condition.negate,
        })
    }

    /// Returns true when this condition matches the tag map.
    pub fn matches(&self, tags: &OsmTags) -> bool {
        let value = tags.get(&self.key);
        let matched = match &self.operator {
            ConditionOperator::Exists(expected) => value.is_some() == *expected,
            ConditionOperator::Value(expected) => value == Some(expected),
            ConditionOperator::Values(expected) => {
                value.map(|value| expected.contains(value)).unwrap_or(false)
            }
            ConditionOperator::Regex(regex) => {
                value.map(|value| regex.is_match(value)).unwrap_or(false)
            }
        };
        if self.negate {
            !matched
        } else {
            matched
        }
    }
}

#[derive(Debug)]
enum ConditionOperator {
    Exists(bool),
    Value(String),
    Values(HashSet<String>),
    Regex(Regex),
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use osmpbfreader::{fileformat, osmformat, Node, OsmId, Ref};
    use protobuf::Message;

    use crate::index::{IndexMode, MemoryNodeIndex};
    use crate::spec::{OsmFilterRules, OsmOutputSpec, OsmProcessingSpec};

    use super::*;

    fn tags(values: &[(&str, &str)]) -> OsmTags {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn osm_tags(values: &[(&str, &str)]) -> OsmPbfTags {
        values
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect()
    }

    fn node(id: i64, lon: f64, lat: f64, tags: OsmPbfTags) -> OsmObj {
        let stored = StoredCoordinate::from_degrees(lon, lat);
        OsmObj::Node(Node {
            id: NodeId(id),
            tags,
            decimicro_lat: stored.decimicro_lat,
            decimicro_lon: stored.decimicro_lon,
        })
    }

    fn way(id: i64, nodes: &[i64], tags: OsmPbfTags) -> OsmObj {
        OsmObj::Way(Way {
            id: WayId(id),
            tags,
            nodes: nodes.iter().copied().map(NodeId).collect(),
        })
    }

    fn relation(id: i64, refs: Vec<(OsmId, &str)>, tags: OsmPbfTags) -> OsmObj {
        OsmObj::Relation(Relation {
            id: RelationId(id),
            tags,
            refs: refs
                .into_iter()
                .map(|(member, role)| Ref {
                    member,
                    role: role.into(),
                })
                .collect(),
        })
    }

    fn spec(types: Vec<OsmElementType>, include: Option<OsmIncludeRules>) -> OsmFilterSpec {
        OsmFilterSpec {
            filter: OsmFilterRules {
                bbox: None,
                types: Some(types),
                include,
                exclude: Vec::new(),
            },
            processing: OsmProcessingSpec::default(),
            output: OsmOutputSpec::default(),
        }
    }

    fn memory_index_options() -> IndexOptions {
        IndexOptions {
            mode: IndexMode::Memory,
            memory_node_limit: 10,
            disk_dir: None,
        }
    }

    fn filter_constructed_objects(
        objects: &[OsmObj],
        spec: OsmFilterSpec,
    ) -> Result<(Vec<OsmFeature>, CollectOsmReport)> {
        let compiled = CompiledOsmFilter::compile(&spec)?;
        let mut node_index = MemoryNodeIndex::new();
        let mut sink = VecFeatureSink::new();
        let mut report = CollectOsmReport::default();

        for object in objects {
            if let OsmObj::Node(node) = object {
                process_node_for_test(
                    node.id,
                    StoredCoordinate::new(node.decimicro_lon, node.decimicro_lat),
                    &node.tags,
                    &compiled,
                    &mut node_index,
                    &mut sink,
                    &mut report,
                )?;
            }
        }

        let mut candidates = Vec::new();
        let mut required_way_ids = HashSet::new();
        if compiled.includes_type(OsmElementType::Relation) {
            for object in objects {
                if let OsmObj::Relation(relation) = object {
                    collect_relation(
                        relation.clone(),
                        &compiled,
                        &mut candidates,
                        &mut required_way_ids,
                        &mut report,
                    );
                }
            }
        }

        let mut relation_way_geometries = HashMap::new();
        if compiled.includes_type(OsmElementType::Way) || !required_way_ids.is_empty() {
            for object in objects {
                if let OsmObj::Way(way) = object {
                    process_way(
                        way,
                        &compiled,
                        &node_index,
                        &required_way_ids,
                        &mut relation_way_geometries,
                        &mut sink,
                        &mut report,
                    )?;
                }
            }
        }

        emit_relations(
            &compiled,
            &candidates,
            &relation_way_geometries,
            &mut sink,
            &mut report,
        )?;

        Ok((sink.features, report))
    }

    fn synthetic_pbf_bytes() -> Vec<u8> {
        let mut string_table = osmformat::StringTable::new();
        for value in [
            "",
            "amenity",
            "school",
            "highway",
            "residential",
            "name",
            "Synthetic Road",
        ] {
            string_table.mut_s().push(value.as_bytes().to_vec());
        }

        let mut dense_nodes = osmformat::DenseNodes::new();
        let mut previous_id = 0_i64;
        let mut previous_lat = 0_i64;
        let mut previous_lon = 0_i64;
        for (id, lat, lon) in [
            (1_i64, 480_000_000_i64, 80_000_000_i64),
            (2, 480_001_000, 80_001_000),
            (3, 480_002_000, 80_002_000),
        ] {
            dense_nodes.id.push(id - previous_id);
            dense_nodes.lat.push(lat - previous_lat);
            dense_nodes.lon.push(lon - previous_lon);
            previous_id = id;
            previous_lat = lat;
            previous_lon = lon;
        }
        dense_nodes.keys_vals = vec![1, 2, 0, 0, 0];

        let mut way = osmformat::Way::new();
        way.set_id(10);
        way.keys = vec![3, 5];
        way.vals = vec![4, 6];
        way.refs = vec![1, 1, 1];

        let mut group = osmformat::PrimitiveGroup::new();
        group.set_dense(dense_nodes);
        group.mut_ways().push(way);

        let mut block = osmformat::PrimitiveBlock::new();
        block.set_stringtable(string_table);
        block.mut_primitivegroup().push(group);

        let mut bytes = Vec::new();
        write_raw_blob(&mut bytes, "OSMData", block.write_to_bytes().unwrap());
        bytes
    }

    fn write_raw_blob(writer: &mut Vec<u8>, field_type: &str, payload: Vec<u8>) {
        let mut blob = fileformat::Blob::new();
        blob.set_raw(payload);
        let blob_bytes = blob.write_to_bytes().unwrap();

        let mut header = fileformat::BlobHeader::new();
        header.set_field_type(field_type.to_owned());
        header.set_datasize(blob_bytes.len().try_into().unwrap());
        let header_bytes = header.write_to_bytes().unwrap();

        let header_len: u32 = header_bytes.len().try_into().unwrap();
        writer.write_all(&header_len.to_be_bytes()).unwrap();
        writer.write_all(&header_bytes).unwrap();
        writer.write_all(&blob_bytes).unwrap();
    }

    #[test]
    fn condition_matches_values_and_negation() {
        let condition = CompiledCondition::compile(&OsmTagCondition {
            key: "amenity".to_owned(),
            exists: None,
            value: None,
            values: Some(vec!["school".to_owned(), "hospital".to_owned()]),
            regex: None,
            negate: false,
        })
        .unwrap();
        assert!(condition.matches(&tags(&[("amenity", "school")])));
        assert!(!condition.matches(&tags(&[("amenity", "cafe")])));

        let condition = CompiledCondition::compile(&OsmTagCondition {
            key: "access".to_owned(),
            exists: None,
            value: Some("private".to_owned()),
            values: None,
            regex: None,
            negate: true,
        })
        .unwrap();
        assert!(!condition.matches(&tags(&[("access", "private")])));
        assert!(condition.matches(&tags(&[("access", "yes")])));
    }

    #[test]
    fn node_filter_emits_point() {
        let (features, _) = filter_constructed_objects(
            &[node(1, 8.7, 48.9, osm_tags(&[("amenity", "school")]))],
            spec(vec![OsmElementType::Node], None),
        )
        .unwrap();
        assert!(matches!(features[0].geometry, Geometry::Point { .. }));
    }

    #[test]
    fn way_filter_resolves_nodes() {
        let (features, _) = filter_constructed_objects(
            &[
                node(1, 0.0, 0.0, osm_tags(&[])),
                node(2, 1.0, 0.0, osm_tags(&[])),
                way(10, &[1, 2], osm_tags(&[("highway", "residential")])),
            ],
            spec(vec![OsmElementType::Way], None),
        )
        .unwrap();
        assert!(matches!(features[0].geometry, Geometry::LineString { .. }));
    }

    #[test]
    fn closed_way_can_emit_polygon() {
        let mut filter_spec = spec(vec![OsmElementType::Way], None);
        filter_spec.output.geometry = OsmGeometryMode::Polygon;
        let (features, _) = filter_constructed_objects(
            &[
                node(1, 0.0, 0.0, osm_tags(&[])),
                node(2, 1.0, 0.0, osm_tags(&[])),
                node(3, 1.0, 1.0, osm_tags(&[])),
                way(10, &[1, 2, 3, 1], osm_tags(&[("building", "yes")])),
            ],
            filter_spec,
        )
        .unwrap();
        assert!(matches!(features[0].geometry, Geometry::Polygon { .. }));
    }

    #[test]
    fn way_geometry_respects_line_and_polygon_modes() {
        let coordinates = vec![
            Coordinate::new(0.0, 0.0).unwrap(),
            Coordinate::new(1.0, 0.0).unwrap(),
            Coordinate::new(1.0, 1.0).unwrap(),
            Coordinate::new(0.0, 0.0).unwrap(),
        ];

        assert!(matches!(
            way_geometry(&coordinates, true, OsmGeometryMode::Full),
            Geometry::LineString { .. }
        ));
        assert!(matches!(
            way_geometry(&coordinates, true, OsmGeometryMode::Polygon),
            Geometry::Polygon { .. }
        ));
    }

    #[test]
    fn coordinates_for_way_returns_none_when_any_node_is_missing() {
        let mut index = MemoryNodeIndex::new();
        index
            .insert(NodeId(1), StoredCoordinate::from_degrees(8.0, 48.0))
            .unwrap();

        let coordinates = coordinates_for_way(&[NodeId(1), NodeId(2)], &index).unwrap();

        assert!(coordinates.is_none());
    }

    #[test]
    fn assemble_relation_reports_missing_members_and_invalid_rings() {
        let candidate = CandidateRelation {
            id: RelationId(1),
            tags: tags(&[("type", "multipolygon")]),
            members: vec![RelationWayMember {
                way_id: WayId(10),
                role: RelationMemberRole::Outer,
            }],
        };
        let missing = HashMap::new();
        assert!(matches!(
            assemble_relation(&candidate, &missing),
            RelationAssemblyResult::MissingMembers
        ));

        let invalid = HashMap::from([(
            WayId(10),
            vec![
                Coordinate::new(0.0, 0.0).unwrap(),
                Coordinate::new(1.0, 0.0).unwrap(),
                Coordinate::new(1.0, 1.0).unwrap(),
            ],
        )]);
        assert!(matches!(
            assemble_relation(&candidate, &invalid),
            RelationAssemblyResult::InvalidRings
        ));
    }

    #[test]
    fn normalize_tags_preserves_deterministic_content() {
        let normalized = normalize_tags(&osm_tags(&[("z", "last"), ("a", "first")]));

        assert_eq!(
            normalized.keys().cloned().collect::<Vec<_>>(),
            vec!["a".to_string(), "z".to_string()]
        );
        assert_eq!(normalized["a"], "first");
        assert_eq!(normalized["z"], "last");
    }

    #[test]
    fn way_with_missing_node_is_counted() {
        let (features, report) = filter_constructed_objects(
            &[
                node(1, 0.0, 0.0, osm_tags(&[])),
                way(10, &[1, 99], osm_tags(&[("highway", "residential")])),
            ],
            spec(vec![OsmElementType::Way], None),
        )
        .unwrap();
        assert!(features.is_empty());
        assert_eq!(report.ways_skipped_missing_nodes, 1);
    }

    #[test]
    fn relation_stitches_reversed_way_fragments() {
        let (features, _) = filter_constructed_objects(
            &[
                node(1, 0.0, 0.0, osm_tags(&[])),
                node(2, 1.0, 0.0, osm_tags(&[])),
                node(3, 1.0, 1.0, osm_tags(&[])),
                node(4, 0.0, 1.0, osm_tags(&[])),
                way(10, &[1, 2, 3], osm_tags(&[])),
                way(11, &[1, 4, 3], osm_tags(&[])),
                relation(
                    20,
                    vec![
                        (OsmId::Way(WayId(10)), "outer"),
                        (OsmId::Way(WayId(11)), "outer"),
                    ],
                    osm_tags(&[("type", "multipolygon")]),
                ),
            ],
            spec(vec![OsmElementType::Relation], None),
        )
        .unwrap();
        assert!(matches!(features[0].geometry, Geometry::Polygon { .. }));
    }

    #[test]
    fn relation_with_multiple_outers_emits_multipolygon() {
        let (features, _) = filter_constructed_objects(
            &[
                node(1, 0.0, 0.0, osm_tags(&[])),
                node(2, 1.0, 0.0, osm_tags(&[])),
                node(3, 1.0, 1.0, osm_tags(&[])),
                node(4, 0.0, 1.0, osm_tags(&[])),
                node(5, 3.0, 3.0, osm_tags(&[])),
                node(6, 4.0, 3.0, osm_tags(&[])),
                node(7, 4.0, 4.0, osm_tags(&[])),
                node(8, 3.0, 4.0, osm_tags(&[])),
                way(10, &[1, 2, 3, 4, 1], osm_tags(&[])),
                way(11, &[5, 6, 7, 8, 5], osm_tags(&[])),
                relation(
                    20,
                    vec![
                        (OsmId::Way(WayId(10)), "outer"),
                        (OsmId::Way(WayId(11)), "outer"),
                    ],
                    osm_tags(&[("type", "multipolygon")]),
                ),
            ],
            spec(vec![OsmElementType::Relation], None),
        )
        .unwrap();
        assert!(matches!(
            features[0].geometry,
            Geometry::MultiPolygon { .. }
        ));
    }

    #[test]
    fn relation_with_hole_assigns_inner_ring() {
        let (features, _) = filter_constructed_objects(
            &[
                node(1, 0.0, 0.0, osm_tags(&[])),
                node(2, 4.0, 0.0, osm_tags(&[])),
                node(3, 4.0, 4.0, osm_tags(&[])),
                node(4, 0.0, 4.0, osm_tags(&[])),
                node(5, 1.0, 1.0, osm_tags(&[])),
                node(6, 2.0, 1.0, osm_tags(&[])),
                node(7, 2.0, 2.0, osm_tags(&[])),
                node(8, 1.0, 2.0, osm_tags(&[])),
                way(10, &[1, 2, 3, 4, 1], osm_tags(&[])),
                way(11, &[5, 6, 7, 8, 5], osm_tags(&[])),
                relation(
                    20,
                    vec![
                        (OsmId::Way(WayId(10)), "outer"),
                        (OsmId::Way(WayId(11)), "inner"),
                    ],
                    osm_tags(&[("type", "multipolygon")]),
                ),
            ],
            spec(vec![OsmElementType::Relation], None),
        )
        .unwrap();
        let Geometry::Polygon { coordinates } = &features[0].geometry else {
            panic!("expected polygon");
        };
        assert_eq!(coordinates.len(), 2);
    }

    #[test]
    fn non_area_relation_is_counted() {
        let (_, report) = filter_constructed_objects(
            &[relation(
                20,
                vec![],
                osm_tags(&[("type", "route"), ("route", "bus")]),
            )],
            spec(vec![OsmElementType::Relation], None),
        )
        .unwrap();
        assert_eq!(report.relations_skipped_non_area, 1);
    }

    #[test]
    fn relation_with_missing_members_is_counted() {
        let (_, report) = filter_constructed_objects(
            &[relation(
                20,
                vec![(OsmId::Way(WayId(10)), "outer")],
                osm_tags(&[("type", "multipolygon")]),
            )],
            spec(vec![OsmElementType::Relation], None),
        )
        .unwrap();
        assert_eq!(report.relations_skipped_missing_members, 1);
    }

    #[test]
    fn relation_with_open_ring_is_counted() {
        let (_, report) = filter_constructed_objects(
            &[
                node(1, 0.0, 0.0, osm_tags(&[])),
                node(2, 1.0, 0.0, osm_tags(&[])),
                node(3, 1.0, 1.0, osm_tags(&[])),
                way(10, &[1, 2, 3], osm_tags(&[])),
                relation(
                    20,
                    vec![(OsmId::Way(WayId(10)), "outer")],
                    osm_tags(&[("type", "multipolygon")]),
                ),
            ],
            spec(vec![OsmElementType::Relation], None),
        )
        .unwrap();
        assert_eq!(report.relations_skipped_invalid_rings, 1);
    }

    #[test]
    fn collect_pbf_bytes_returns_geo_collection_metadata() {
        let collected = collect_osm_pbf_bytes(CollectOsmBytesOptions {
            input: &synthetic_pbf_bytes(),
            spec: OsmFilterSpec::from_inline("amenity=school").unwrap(),
            index_options: memory_index_options(),
        })
        .unwrap();
        let geo = collected.into_geo_feature_collection();
        assert_eq!(geo.features[0].id.as_deref(), Some("node/1"));
        assert_eq!(geo.features[0].properties["amenity"], "school");
    }

    #[test]
    fn collect_pbf_bytes_geojson_conversion_is_valid() {
        let collected = collect_osm_pbf_bytes(CollectOsmBytesOptions {
            input: &synthetic_pbf_bytes(),
            spec: OsmFilterSpec::from_inline("highway=residential").unwrap(),
            index_options: memory_index_options(),
        })
        .unwrap();
        let geo = collected.into_geo_feature_collection();
        let geojson = geo_io_geojson::to_geojson_feature_collection(&geo.features);
        let value = serde_json::to_value(&geojson.features[0]).unwrap();
        assert_eq!(value["id"], "way/10");
    }
}
