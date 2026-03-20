use std::env;
use std::path::{Path, PathBuf};

use ndarray::Array2;
use ort::session::Session;
use ort::value::Tensor;
use serde::Deserialize;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams, TruncationStrategy};
use tracing::info;

use super::{Classification, Complexity};

#[derive(Debug, Clone)]
pub(crate) struct DispatchArtifacts {
    root: PathBuf,
}

impl DispatchArtifacts {
    pub(crate) fn from_root(path: impl AsRef<Path>) -> Result<Self, String> {
        let root = path.as_ref().to_path_buf();
        let artifacts = Self { root };

        for required in [
            artifacts.model_path(),
            artifacts.labels_path(),
            artifacts.prototypes_path(),
            artifacts.tokenizer_path(),
        ] {
            if !required.exists() {
                return Err(format!("missing dispatch artifact: {}", required.display()));
            }
        }

        Ok(artifacts)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    fn model_path(&self) -> PathBuf {
        self.root.join("dispatch.onnx")
    }

    fn labels_path(&self) -> PathBuf {
        self.root.join("dispatch_labels.json")
    }

    fn prototypes_path(&self) -> PathBuf {
        self.root.join("dispatch_prototypes.json")
    }

    fn tokenizer_path(&self) -> PathBuf {
        self.root.join("tokenizer.json")
    }
}

#[derive(Debug, Deserialize)]
struct LabelMetadata {
    max_length: usize,
    complexity_labels: Vec<String>,
}

pub(crate) struct DispatchModel {
    session: Session,
    tokenizer: Tokenizer,
    max_length: usize,
    complexity_labels: Vec<String>,
    prototypes: Vec<(String, Vec<f32>)>,
}

impl DispatchModel {
    pub(crate) fn load(artifacts: DispatchArtifacts) -> Result<Self, String> {
        let labels: LabelMetadata = serde_json::from_str(
            &std::fs::read_to_string(artifacts.labels_path())
                .map_err(|error| format!("failed to read labels: {error}"))?,
        )
        .map_err(|error| format!("failed to parse labels: {error}"))?;

        let prototypes = load_prototypes(&artifacts.prototypes_path())?;
        let session = Session::builder()
            .map_err(|error| format!("failed to create ONNX session builder: {error}"))?
            .commit_from_file(artifacts.model_path())
            .map_err(|error| format!("failed to load ONNX model: {error}"))?;

        let mut tokenizer = Tokenizer::from_file(artifacts.tokenizer_path())
            .map_err(|error| format!("failed to load tokenizer: {error}"))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: labels.max_length,
                strategy: TruncationStrategy::LongestFirst,
                ..Default::default()
            }))
            .map_err(|error| format!("failed to configure tokenizer truncation: {error}"))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(labels.max_length),
            ..Default::default()
        }));

        info!(
            artifact_root = %artifacts.root().display(),
            max_length = labels.max_length,
            "Loaded dispatch classifier artifacts"
        );

        Ok(Self {
            session,
            tokenizer,
            max_length: labels.max_length,
            complexity_labels: labels.complexity_labels,
            prototypes,
        })
    }

    pub(crate) fn classify(&mut self, input: &str) -> Result<Classification, String> {
        let encoding = self
            .tokenizer
            .encode(input, true)
            .map_err(|error| format!("failed to tokenize dispatch input: {error}"))?;

        let input_ids = to_row_i64(encoding.get_ids(), self.max_length)?;
        let attention_mask = to_row_i64(encoding.get_attention_mask(), self.max_length)?;
        let type_ids = if encoding.get_type_ids().is_empty() {
            vec![0_i64; self.max_length]
        } else {
            pad_or_truncate(encoding.get_type_ids(), self.max_length)
                .into_iter()
                .map(i64::from)
                .collect()
        };
        let type_ids = Array2::from_shape_vec((1, self.max_length), type_ids)
            .map_err(|error| error.to_string())?;

        let outputs = self
            .session
            .run(ort::inputs! {
                "input_ids" => Tensor::from_array(input_ids)
                    .map_err(|error| format!("failed to build input_ids tensor: {error}"))?,
                "attention_mask" => Tensor::from_array(attention_mask)
                    .map_err(|error| format!("failed to build attention_mask tensor: {error}"))?,
                "token_type_ids" => Tensor::from_array(type_ids)
                    .map_err(|error| format!("failed to build token_type_ids tensor: {error}"))?,
            })
            .map_err(|error| format!("dispatch inference failed: {error}"))?;

        let domain_embed = extract_output(&outputs, "domain_embed")?;
        let complexity_logits = extract_output(&outputs, "complexity_logits")?;
        let sensitive_logits = extract_output(&outputs, "sensitive_logits")?;
        let injection_logits = extract_output(&outputs, "injection_logits")?;

        let (domain_label, domain_confidence) = nearest_domain(&domain_embed, &self.prototypes);
        let complexity = map_complexity(&self.complexity_labels, &complexity_logits);
        let sensitive = sigmoid(sensitive_logits.first().copied().unwrap_or_default()) >= 0.5;
        let injection_score = sigmoid(injection_logits.first().copied().unwrap_or_default());

        Ok(Classification {
            domain_label,
            domain_confidence,
            complexity,
            sensitive,
            injection_score,
        })
    }
}

pub(crate) fn discover_default_artifacts() -> Option<DispatchArtifacts> {
    let mut candidates = Vec::new();

    if let Ok(path) = env::var("ROVE_DISPATCH_ARTIFACTS") {
        candidates.push(PathBuf::from(path));
    }

    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".rove/brains/dispatch"));
    }

    let workspace_relative =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../brains/task-classifier/artifacts");
    candidates.push(workspace_relative);

    candidates
        .into_iter()
        .find_map(|candidate| DispatchArtifacts::from_root(candidate).ok())
}

fn load_prototypes(path: &Path) -> Result<Vec<(String, Vec<f32>)>, String> {
    let raw: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read prototypes: {error}"))?,
    )
    .map_err(|error| format!("failed to parse prototypes: {error}"))?;

    raw.into_iter()
        .map(|(label, value)| {
            let vector: Vec<f32> =
                serde_json::from_value(value).map_err(|error| format!("bad prototype: {error}"))?;
            Ok((label, vector))
        })
        .collect()
}

fn pad_or_truncate(values: &[u32], length: usize) -> Vec<u32> {
    let mut padded = values.iter().copied().take(length).collect::<Vec<_>>();
    if padded.len() < length {
        padded.resize(length, 0);
    }
    padded
}

fn to_row_i64(values: &[u32], length: usize) -> Result<Array2<i64>, String> {
    let row = pad_or_truncate(values, length)
        .into_iter()
        .map(i64::from)
        .collect::<Vec<_>>();
    Array2::from_shape_vec((1, length), row).map_err(|error| error.to_string())
}

fn extract_output(
    outputs: &ort::session::SessionOutputs<'_>,
    name: &str,
) -> Result<Vec<f32>, String> {
    let value = outputs
        .get(name)
        .ok_or_else(|| format!("missing ONNX output: {name}"))?;
    let (_, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|error| format!("failed to decode ONNX output {name}: {error}"))?;
    Ok(data.to_vec())
}

fn nearest_domain(embed: &[f32], prototypes: &[(String, Vec<f32>)]) -> (String, f32) {
    let embed_norm = normalize(embed);
    let mut best_label = String::from("general");
    let mut best_score = -1.0_f32;

    for (label, prototype) in prototypes {
        let score = cosine_similarity(&embed_norm, &normalize(prototype));
        if score > best_score {
            best_score = score;
            best_label = label.clone();
        }
    }

    (best_label, best_score.max(0.0))
}

fn normalize(values: &[f32]) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return values.to_vec();
    }
    values.iter().map(|value| value / norm).collect()
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum()
}

fn map_complexity(labels: &[String], logits: &[f32]) -> Complexity {
    let best_idx = logits
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(idx, _)| idx)
        .unwrap_or_default();

    match labels
        .get(best_idx)
        .map(|label| label.as_str())
        .unwrap_or("simple")
    {
        "complex" => Complexity::Complex,
        "medium" => Complexity::Medium,
        _ => Complexity::Simple,
    }
}

fn sigmoid(logit: f32) -> f32 {
    1.0 / (1.0 + (-logit).exp())
}
