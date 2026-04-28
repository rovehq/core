use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
use extism_pdk::*;

#[derive(Debug, Deserialize)]
pub struct RunInput {
    pub input: String,
}

#[derive(Debug, Serialize)]
pub struct RunOutput {
    pub echoed: String,
}

pub fn run_impl(input: RunInput) -> RunOutput {
    RunOutput {
        echoed: input.input,
    }
}

#[cfg(target_arch = "wasm32")]
#[plugin_fn]
pub fn run(Json(input): Json<RunInput>) -> FnResult<Json<RunOutput>> {
    Ok(Json(run_impl(input)))
}
