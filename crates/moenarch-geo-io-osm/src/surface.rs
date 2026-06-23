//! Library-owned runtime surface for `geo-io-osm`.

use base64::prelude::*;
use runtime_core::{
    OperationId, PackageSurface, RuntimeCapabilities, SurfaceOperation, SurfaceRequest,
    SurfaceResponse,
};
use serde::Deserialize;

use crate::{
    collect_osm_pbf_bytes, CollectOsmBytesOptions, IndexMode, IndexOptions, OsmFilterSpec,
};

/// Returns the package surface exposed by every transport wrapper.
pub fn package_surface() -> PackageSurface {
    PackageSurface {
        library: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: RuntimeCapabilities::pure_rust(),
        operations: vec![
            operation(
                "describe",
                "Describe package",
                "OpenStreetMap PBF import adapters for geo-core domain types.",
                serde_json::json!({"includeOperations": true}),
            ),
            operation(
                "osm.validateSpec",
                "Validate OSM spec",
                "Validates an OSM PBF filter specification.",
                serde_json::json!({"spec": {"filter": {"types": ["node", "way"]}}}),
            ),
            operation(
                "osm.filterSummary",
                "OSM filter summary",
                "Summarizes the effective OSM PBF filter configuration.",
                serde_json::json!({"spec": {"filter": {"bbox": [8.5, 48.8, 9.3, 49.2]}}}),
            ),
            operation(
                "osm.filterPbfBase64",
                "Filter OSM PBF",
                "Filters base64-encoded OSM PBF bytes into GeoJSON-shaped features.",
                serde_json::json!({
                    "pbfBase64": "",
                    "spec": {
                        "filter": {
                            "types": ["node", "way"],
                            "include": {"any": [{"key": "amenity", "values": ["school", "hospital"]}]}
                        },
                        "output": {"geometry": "full"}
                    }
                }),
            ),
        ],
    }
}

fn operation(
    id: &str,
    name: &str,
    description: &str,
    example_request: serde_json::Value,
) -> SurfaceOperation {
    SurfaceOperation {
        id: OperationId::new(id),
        name: name.to_string(),
        description: Some(description.to_string()),
        curation: runtime_core::SurfaceOperationCuration::from_operation_id(id),
        input_schema: serde_json::json!({"type": "object", "additionalProperties": true, "xOperationCategory": runtime_core::operation_category(id)}),
        output_schema: serde_json::json!({"type": "object", "xOperationCategory": runtime_core::operation_category(id)}),
        example_request,
        wasm_supported: true,
        server_supported: true,
    }
}

/// Runs one library-owned operation.
pub fn run_surface_operation(request: SurfaceRequest) -> Result<SurfaceResponse, String> {
    let operation = request.operation.clone();
    let value = match request.operation.as_str() {
        "describe" | "osm.describe" => describe_value(request.input),
        "osm.validateSpec" => validate_spec_value(parse_input(request.input)?)?,
        "osm.filterSummary" => filter_summary_value(parse_input(request.input)?)?,
        "osm.filterPbfBase64" => filter_pbf_base64_value(parse_input(request.input)?)?,
        operation => {
            return Err(format!(
                "unsupported operation `{operation}` for {}",
                env!("CARGO_PKG_NAME")
            ));
        }
    };
    Ok(SurfaceResponse {
        operation,
        value,
        diagnostics: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn describe_value(input: serde_json::Value) -> serde_json::Value {
    let surface = package_surface();
    serde_json::json!({
        "library": surface.library,
        "version": surface.version,
        "operationCount": surface.operations.len(),
        "operations": surface
            .operations
            .iter()
            .map(|operation| operation.id.as_str())
            .collect::<Vec<_>>(),
        "input": input
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpecRequest {
    #[serde(default)]
    spec: OsmFilterSpec,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilterPbfBase64Request {
    pbf_base64: String,
    #[serde(default)]
    spec: OsmFilterSpec,
    #[serde(default)]
    index: Option<SurfaceIndexOptions>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SurfaceIndexOptions {
    mode: Option<IndexMode>,
    memory_node_limit: Option<usize>,
}

fn validate_spec_value(request: SpecRequest) -> Result<serde_json::Value, String> {
    request.spec.validate().map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "valid": true,
        "spec": request.spec
    }))
}

fn filter_summary_value(request: SpecRequest) -> Result<serde_json::Value, String> {
    request.spec.validate().map_err(|error| error.to_string())?;
    let types = request
        .spec
        .filter
        .types
        .clone()
        .unwrap_or_else(|| vec![crate::OsmElementType::Node, crate::OsmElementType::Way]);
    Ok(serde_json::json!({
        "valid": true,
        "types": types,
        "hasBbox": request.spec.filter.bbox.is_some(),
        "includeAny": request.spec.filter.include.as_ref().map(|rules| rules.any.len()).unwrap_or(0),
        "includeAll": request.spec.filter.include.as_ref().map(|rules| rules.all.len()).unwrap_or(0),
        "exclude": request.spec.filter.exclude.len(),
        "geometry": request.spec.output.geometry,
        "index": request.spec.processing.index
    }))
}

fn filter_pbf_base64_value(request: FilterPbfBase64Request) -> Result<serde_json::Value, String> {
    request.spec.validate().map_err(|error| error.to_string())?;
    let pbf_base64 = request
        .pbf_base64
        .trim()
        .rsplit_once(',')
        .map(|(_, payload)| payload)
        .unwrap_or_else(|| request.pbf_base64.trim());
    let pbf = BASE64_STANDARD
        .decode(pbf_base64)
        .map_err(|error| format!("invalid pbfBase64: {error}"))?;
    let index_options = request.index.map_or_else(
        || IndexOptions::from_spec(&request.spec.processing.index),
        |index| IndexOptions {
            mode: index.mode.unwrap_or(request.spec.processing.index.mode),
            memory_node_limit: index
                .memory_node_limit
                .unwrap_or(request.spec.processing.index.memory_node_limit),
            disk_dir: request.spec.processing.index.disk_dir.clone(),
        },
    );
    let collected = collect_osm_pbf_bytes(CollectOsmBytesOptions {
        input: &pbf,
        spec: request.spec,
        index_options,
    })
    .map_err(|error| error.to_string())?;
    let report = collected.report.clone();
    let geo = collected.into_geo_feature_collection();
    let feature_collection = geo_io_geojson::to_geojson_feature_collection(&geo.features);
    Ok(serde_json::json!({
        "featureCount": feature_collection.features.len(),
        "features": feature_collection.features,
        "report": report
    }))
}

fn parse_input<T: for<'de> Deserialize<'de>>(input: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("invalid request: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use base64::prelude::*;
    use osmpbfreader::{fileformat, osmformat};
    use protobuf::Message;

    use super::*;

    #[test]
    fn validates_spec() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("osm.validateSpec"),
            input: serde_json::json!({"spec": {"filter": {"types": ["node"]}}}),
        })
        .unwrap();
        assert_eq!(response.value["valid"], true);
    }

    #[test]
    fn summarizes_spec() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("osm.filterSummary"),
            input: serde_json::json!({"spec": {"filter": {"bbox": [8.0, 48.0, 9.0, 49.0]}}}),
        })
        .unwrap();
        assert_eq!(response.value["hasBbox"], true);
    }

    #[test]
    fn filters_base64_pbf_into_geojson_features() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("osm.filterPbfBase64"),
            input: serde_json::json!({
                "pbfBase64": BASE64_STANDARD.encode(synthetic_pbf_bytes()),
                "spec": {"filter": {"types": ["node"], "include": {"all": [{"key": "amenity", "value": "school"}]}}}
            }),
        })
        .unwrap();

        assert_eq!(response.value["featureCount"], 1);
        assert_eq!(response.value["features"][0]["id"], "node/1");
        assert_eq!(
            response.value["features"][0]["properties"]["amenity"],
            "school"
        );
        assert_eq!(response.value["report"]["objectsCollected"], 1);
    }

    fn synthetic_pbf_bytes() -> Vec<u8> {
        let mut string_table = osmformat::StringTable::new();
        for value in ["", "amenity", "school"] {
            string_table.mut_s().push(value.as_bytes().to_vec());
        }

        let mut dense_nodes = osmformat::DenseNodes::new();
        dense_nodes.id = vec![1];
        dense_nodes.lat = vec![480_000_000];
        dense_nodes.lon = vec![80_000_000];
        dense_nodes.keys_vals = vec![1, 2, 0];

        let mut group = osmformat::PrimitiveGroup::new();
        group.set_dense(dense_nodes);

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
}
