use std::collections::HashMap;
use std::path::PathBuf;

use geo_core::{Coordinate, GeoError, Result};
use osmpbfreader::NodeId;
#[cfg(feature = "disk-index")]
use redb::{Database, ReadableDatabase, TableDefinition};
#[cfg(feature = "disk-index")]
use tempfile::{NamedTempFile, TempDir};

pub use crate::spec::IndexMode;
use crate::spec::OsmIndexSpec;

#[cfg(feature = "disk-index")]
const NODE_TABLE: TableDefinition<i64, &[u8]> = TableDefinition::new("nodes");
#[cfg(feature = "disk-index")]
const STORED_COORDINATE_BYTES: usize = 8;

#[cfg(not(feature = "disk-index"))]
fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}

#[cfg(feature = "disk-index")]
fn source_error(message: impl Into<String>) -> GeoError {
    GeoError::source(message)
}

#[derive(Debug, Clone)]
/// Node-index runtime options.
pub struct IndexOptions {
    /// Index backend mode.
    pub mode: IndexMode,
    /// Number of nodes kept in memory before auto spill.
    pub memory_node_limit: usize,
    /// Optional disk index directory.
    pub disk_dir: Option<PathBuf>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            mode: IndexMode::Auto,
            memory_node_limit: 5_000_000,
            disk_dir: None,
        }
    }
}

impl IndexOptions {
    /// Builds options from a spec index section.
    pub fn from_spec(spec: &OsmIndexSpec) -> Self {
        Self {
            mode: spec.mode,
            memory_node_limit: spec.memory_node_limit,
            disk_dir: spec.disk_dir.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
/// Actual node-index backend used by a run.
pub enum IndexBackend {
    /// In-memory hash map.
    Memory,
    /// Disk-backed redb table.
    Disk,
}

impl IndexBackend {
    /// Returns the lower-case backend name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Disk => "disk",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Compact coordinate storage in OSM decimicrodegrees.
pub struct StoredCoordinate {
    /// Longitude in 1e-7 degree units.
    pub decimicro_lon: i32,
    /// Latitude in 1e-7 degree units.
    pub decimicro_lat: i32,
}

impl StoredCoordinate {
    /// Creates a stored coordinate.
    pub fn new(decimicro_lon: i32, decimicro_lat: i32) -> Self {
        Self {
            decimicro_lon,
            decimicro_lat,
        }
    }

    /// Converts degree coordinates to decimicrodegree storage.
    pub fn from_degrees(lon: f64, lat: f64) -> Self {
        Self {
            decimicro_lon: (lon / 1e-7).round() as i32,
            decimicro_lat: (lat / 1e-7).round() as i32,
        }
    }

    /// Converts stored coordinates into a validated `geo-core` coordinate.
    pub fn to_coordinate(self) -> Result<Coordinate> {
        Coordinate::new(
            self.decimicro_lon as f64 * 1e-7,
            self.decimicro_lat as f64 * 1e-7,
        )
    }

    #[cfg(feature = "disk-index")]
    fn to_bytes(self) -> [u8; STORED_COORDINATE_BYTES] {
        let mut bytes = [0_u8; STORED_COORDINATE_BYTES];
        bytes[..4].copy_from_slice(&self.decimicro_lon.to_le_bytes());
        bytes[4..].copy_from_slice(&self.decimicro_lat.to_le_bytes());
        bytes
    }

    #[cfg(feature = "disk-index")]
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != STORED_COORDINATE_BYTES {
            return None;
        }
        Some(Self {
            decimicro_lon: i32::from_le_bytes(bytes[..4].try_into().ok()?),
            decimicro_lat: i32::from_le_bytes(bytes[4..].try_into().ok()?),
        })
    }
}

/// Node coordinate index abstraction.
pub trait NodeIndex {
    /// Inserts one node coordinate.
    fn insert(&mut self, node_id: NodeId, coordinate: StoredCoordinate) -> Result<()>;

    /// Inserts a batch of node coordinates.
    fn insert_batch(&mut self, entries: &[(NodeId, StoredCoordinate)]) -> Result<()> {
        for (node_id, coordinate) in entries {
            self.insert(*node_id, *coordinate)?;
        }
        Ok(())
    }

    /// Fetches a node coordinate.
    fn get(&self, node_id: NodeId) -> Result<Option<StoredCoordinate>>;
    /// Returns the active backend.
    fn backend(&self) -> IndexBackend;
    /// Returns number of indexed nodes.
    fn len(&self) -> usize;

    /// Returns true when empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Default)]
/// In-memory node coordinate index.
pub struct MemoryNodeIndex {
    nodes: HashMap<NodeId, StoredCoordinate>,
}

impl MemoryNodeIndex {
    /// Creates an empty memory index.
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(feature = "disk-index")]
    fn drain(self) -> HashMap<NodeId, StoredCoordinate> {
        self.nodes
    }
}

impl NodeIndex for MemoryNodeIndex {
    fn insert(&mut self, node_id: NodeId, coordinate: StoredCoordinate) -> Result<()> {
        self.nodes.insert(node_id, coordinate);
        Ok(())
    }

    fn insert_batch(&mut self, entries: &[(NodeId, StoredCoordinate)]) -> Result<()> {
        self.nodes.reserve(entries.len());
        for (node_id, coordinate) in entries {
            self.nodes.insert(*node_id, *coordinate);
        }
        Ok(())
    }

    fn get(&self, node_id: NodeId) -> Result<Option<StoredCoordinate>> {
        Ok(self.nodes.get(&node_id).copied())
    }

    fn backend(&self) -> IndexBackend {
        IndexBackend::Memory
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }
}

#[cfg(feature = "disk-index")]
#[derive(Debug)]
/// Disk-backed redb node coordinate index.
pub struct RedbNodeIndex {
    database: Database,
    path: PathBuf,
    _temp_file: Option<NamedTempFile>,
    _temp_dir: Option<TempDir>,
    len: usize,
}

#[cfg(feature = "disk-index")]
impl RedbNodeIndex {
    /// Creates a disk-backed node index.
    pub fn create(options: &IndexOptions) -> Result<Self> {
        let (path, temp_file, temp_dir) = index_path(options)?;
        let database = Database::create(&path).map_err(|source| {
            source_error(format!(
                "node index failed for `{}`: {source}",
                path.display()
            ))
        })?;
        let index = Self {
            database,
            path,
            _temp_file: temp_file,
            _temp_dir: temp_dir,
            len: 0,
        };
        index.create_table()?;
        Ok(index)
    }

    fn create_table(&self) -> Result<()> {
        let write_txn = self.database.begin_write().map_err(|source| {
            source_error(format!(
                "node index failed for `{}`: {source}",
                self.path.display()
            ))
        })?;
        {
            write_txn.open_table(NODE_TABLE).map_err(|source| {
                source_error(format!(
                    "node index failed for `{}`: {source}",
                    self.path.display()
                ))
            })?;
        }
        write_txn.commit().map_err(|source| {
            source_error(format!(
                "node index failed for `{}`: {source}",
                self.path.display()
            ))
        })
    }
}

#[cfg(feature = "disk-index")]
impl NodeIndex for RedbNodeIndex {
    fn insert(&mut self, node_id: NodeId, coordinate: StoredCoordinate) -> Result<()> {
        self.insert_batch(&[(node_id, coordinate)])
    }

    fn insert_batch(&mut self, entries: &[(NodeId, StoredCoordinate)]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let write_txn = self.database.begin_write().map_err(|source| {
            source_error(format!(
                "node index failed for `{}`: {source}",
                self.path.display()
            ))
        })?;
        {
            let mut table = write_txn.open_table(NODE_TABLE).map_err(|source| {
                source_error(format!(
                    "node index failed for `{}`: {source}",
                    self.path.display()
                ))
            })?;
            for (node_id, coordinate) in entries {
                let bytes = coordinate.to_bytes();
                if table
                    .insert(node_id.0, bytes.as_slice())
                    .map_err(|source| {
                        source_error(format!(
                            "node index failed for `{}`: {source}",
                            self.path.display()
                        ))
                    })?
                    .is_none()
                {
                    self.len += 1;
                }
            }
        }
        write_txn.commit().map_err(|source| {
            source_error(format!(
                "node index failed for `{}`: {source}",
                self.path.display()
            ))
        })
    }

    fn get(&self, node_id: NodeId) -> Result<Option<StoredCoordinate>> {
        let read_txn = self.database.begin_read().map_err(|source| {
            source_error(format!(
                "node index failed for `{}`: {source}",
                self.path.display()
            ))
        })?;
        let table = read_txn.open_table(NODE_TABLE).map_err(|source| {
            source_error(format!(
                "node index failed for `{}`: {source}",
                self.path.display()
            ))
        })?;
        table
            .get(node_id.0)
            .map_err(|source| {
                source_error(format!(
                    "node index failed for `{}`: {source}",
                    self.path.display()
                ))
            })?
            .map(|value| {
                StoredCoordinate::from_bytes(value.value()).ok_or_else(|| {
                    source_error(format!(
                        "node index failed for `{}`: stored coordinate has invalid byte length",
                        self.path.display()
                    ))
                })
            })
            .transpose()
    }

    fn backend(&self) -> IndexBackend {
        IndexBackend::Disk
    }

    fn len(&self) -> usize {
        self.len
    }
}

#[derive(Debug)]
/// Auto-spilling node index.
pub struct AutoNodeIndex {
    options: IndexOptions,
    inner: AutoNodeIndexInner,
}

#[derive(Debug)]
enum AutoNodeIndexInner {
    Memory(MemoryNodeIndex),
    #[cfg(feature = "disk-index")]
    Disk(RedbNodeIndex),
}

impl AutoNodeIndex {
    /// Creates an auto node index.
    pub fn create(options: IndexOptions) -> Result<Self> {
        let inner = match options.mode {
            IndexMode::Memory | IndexMode::Auto => {
                AutoNodeIndexInner::Memory(MemoryNodeIndex::new())
            }
            #[cfg(feature = "disk-index")]
            IndexMode::Disk => AutoNodeIndexInner::Disk(RedbNodeIndex::create(&options)?),
            #[cfg(not(feature = "disk-index"))]
            IndexMode::Disk => {
                return Err(invalid_argument(
                    "disk-backed node indexes are not supported in this build",
                ));
            }
        };
        Ok(Self { options, inner })
    }

    #[cfg(feature = "disk-index")]
    fn spill_to_disk(&mut self) -> Result<()> {
        let AutoNodeIndexInner::Memory(memory) = std::mem::replace(
            &mut self.inner,
            AutoNodeIndexInner::Memory(MemoryNodeIndex::new()),
        ) else {
            return Ok(());
        };
        let mut disk = RedbNodeIndex::create(&self.options)?;
        let entries: Vec<_> = memory.drain().into_iter().collect();
        disk.insert_batch(&entries)?;
        self.inner = AutoNodeIndexInner::Disk(disk);
        Ok(())
    }
}

impl NodeIndex for AutoNodeIndex {
    fn insert(&mut self, node_id: NodeId, coordinate: StoredCoordinate) -> Result<()> {
        match &mut self.inner {
            AutoNodeIndexInner::Memory(memory) => {
                memory.insert(node_id, coordinate)?;
                if self.options.mode == IndexMode::Auto
                    && memory.len() > self.options.memory_node_limit
                {
                    #[cfg(feature = "disk-index")]
                    self.spill_to_disk()?;
                }
                Ok(())
            }
            #[cfg(feature = "disk-index")]
            AutoNodeIndexInner::Disk(disk) => disk.insert(node_id, coordinate),
        }
    }

    fn insert_batch(&mut self, entries: &[(NodeId, StoredCoordinate)]) -> Result<()> {
        match &mut self.inner {
            AutoNodeIndexInner::Memory(memory) => {
                memory.insert_batch(entries)?;
                if self.options.mode == IndexMode::Auto
                    && memory.len() > self.options.memory_node_limit
                {
                    #[cfg(feature = "disk-index")]
                    self.spill_to_disk()?;
                }
                Ok(())
            }
            #[cfg(feature = "disk-index")]
            AutoNodeIndexInner::Disk(disk) => disk.insert_batch(entries),
        }
    }

    fn get(&self, node_id: NodeId) -> Result<Option<StoredCoordinate>> {
        match &self.inner {
            AutoNodeIndexInner::Memory(memory) => memory.get(node_id),
            #[cfg(feature = "disk-index")]
            AutoNodeIndexInner::Disk(disk) => disk.get(node_id),
        }
    }

    fn backend(&self) -> IndexBackend {
        match &self.inner {
            AutoNodeIndexInner::Memory(memory) => memory.backend(),
            #[cfg(feature = "disk-index")]
            AutoNodeIndexInner::Disk(disk) => disk.backend(),
        }
    }

    fn len(&self) -> usize {
        match &self.inner {
            AutoNodeIndexInner::Memory(memory) => memory.len(),
            #[cfg(feature = "disk-index")]
            AutoNodeIndexInner::Disk(disk) => disk.len(),
        }
    }
}

#[cfg(feature = "disk-index")]
fn index_path(options: &IndexOptions) -> Result<(PathBuf, Option<NamedTempFile>, Option<TempDir>)> {
    if let Some(dir) = &options.disk_dir {
        std::fs::create_dir_all(dir).map_err(GeoError::Io)?;
        let temp_file = NamedTempFile::new_in(dir).map_err(GeoError::Io)?;
        let path = temp_file.path().to_path_buf();
        Ok((path, Some(temp_file), None))
    } else {
        let temp_dir = tempfile::tempdir().map_err(GeoError::Io)?;
        let path = temp_dir.path().join("nodes.redb");
        Ok((path, None, Some(temp_dir)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_index_round_trips_coordinates() {
        let mut index = MemoryNodeIndex::new();
        index
            .insert(NodeId(1), StoredCoordinate::new(87_000_000, 489_000_000))
            .unwrap();
        assert_eq!(
            index.get(NodeId(1)).unwrap(),
            Some(StoredCoordinate::new(87_000_000, 489_000_000))
        );
        assert_eq!(index.backend(), IndexBackend::Memory);
    }

    #[cfg(feature = "disk-index")]
    #[test]
    fn redb_index_round_trips_coordinates() {
        let options = IndexOptions {
            mode: IndexMode::Disk,
            memory_node_limit: 1,
            disk_dir: None,
        };
        let mut index = RedbNodeIndex::create(&options).unwrap();
        index
            .insert(NodeId(2), StoredCoordinate::new(1_000_000, 2_000_000))
            .unwrap();
        assert_eq!(
            index.get(NodeId(2)).unwrap(),
            Some(StoredCoordinate::new(1_000_000, 2_000_000))
        );
        assert_eq!(index.backend(), IndexBackend::Disk);
    }

    #[cfg(feature = "disk-index")]
    #[test]
    fn auto_index_spills_after_threshold() {
        let options = IndexOptions {
            mode: IndexMode::Auto,
            memory_node_limit: 1,
            disk_dir: None,
        };
        let mut index = AutoNodeIndex::create(options).unwrap();
        index
            .insert_batch(&[
                (NodeId(1), StoredCoordinate::new(1, 1)),
                (NodeId(2), StoredCoordinate::new(2, 2)),
            ])
            .unwrap();
        assert_eq!(index.backend(), IndexBackend::Disk);
        assert_eq!(
            index.get(NodeId(1)).unwrap(),
            Some(StoredCoordinate::new(1, 1))
        );
    }

    #[cfg(not(feature = "disk-index"))]
    #[test]
    fn disk_index_is_rejected_without_disk_feature() {
        let options = IndexOptions {
            mode: IndexMode::Disk,
            memory_node_limit: 1,
            disk_dir: None,
        };
        assert!(AutoNodeIndex::create(options).is_err());
    }

    #[cfg(not(feature = "disk-index"))]
    #[test]
    fn auto_index_stays_in_memory_without_disk_feature() {
        let options = IndexOptions {
            mode: IndexMode::Auto,
            memory_node_limit: 1,
            disk_dir: None,
        };
        let mut index = AutoNodeIndex::create(options).unwrap();
        index
            .insert_batch(&[
                (NodeId(1), StoredCoordinate::new(1, 1)),
                (NodeId(2), StoredCoordinate::new(2, 2)),
            ])
            .unwrap();
        assert_eq!(index.backend(), IndexBackend::Memory);
    }
}
