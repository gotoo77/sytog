//! Narrow serialized façade for browser consumers.

use sytog_capabilities::{JobRequirement, NodeOffer, rank, validate_nodes};
use wasm_bindgen::prelude::*;

/// Match serialized V0 requirements without exposing internal Rust layouts.
///
/// # Errors
///
/// Returns a JavaScript string error when either input is invalid JSON or the
/// result cannot be serialized.
#[wasm_bindgen]
pub fn match_capabilities(job_json: &str, nodes_json: &str) -> Result<String, String> {
    let job: JobRequirement =
        serde_json::from_str(job_json).map_err(|error| format!("invalid job: {error}"))?;
    let nodes: Vec<NodeOffer> =
        serde_json::from_str(nodes_json).map_err(|error| format!("invalid nodes: {error}"))?;
    job.validate()
        .map_err(|error| format!("invalid job: {error}"))?;
    validate_nodes(&nodes).map_err(|error| format!("invalid nodes: {error}"))?;
    serde_json::to_string(&rank(&job, &nodes))
        .map_err(|error| format!("cannot serialize result: {error}"))
}
