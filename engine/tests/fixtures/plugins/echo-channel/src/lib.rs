use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
use extism_pdk::*;

#[derive(Debug, Deserialize)]
pub struct DeliverInput {
    pub input: String,
}

#[derive(Debug, Serialize)]
pub struct DeliverOutput {
    pub preview: String,
}

pub fn deliver_impl(input: DeliverInput) -> DeliverOutput {
    DeliverOutput {
        preview: format!("channel: {}", input.input),
    }
}

#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn deliver(Json(input): Json<DeliverInput>) -> FnResult<Json<DeliverOutput>> {
    Ok(Json(deliver_impl(input)))
}
