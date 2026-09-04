#![forbid(unsafe_code)]

//! Validated DeepFilterNet adapter for the pinned low-latency model.

mod model_asset;
mod processor;

pub use model_asset::{
    ModelAsset, PINNED_UPSTREAM_REVISION, SUPPORTED_MANIFEST_SCHEMA, default_model_path,
};
pub use processor::DfnNoiseReducer;
