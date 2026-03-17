//! Project Memory System
//!
//! Provides context about the user's workspace, scanning directories, identifying common
//! project structures, and packing relevant files into context for the LLM.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

/// A brief summary of the project workspace
#[derive(Debug, Clone)]
pub struct ProjectMemory {
    pub workspace_path: PathBuf,
    pub top_level_files: Vec<String>,
    pub likely_languages: Vec<String>,
}

impl ProjectMemory {
    /// Create a new ProjectMemory by scanning the given workspace path
    pub async fn scan(workspace: &Path) -> Result<Self> {
        let mut top_level_files = Vec::new();
        let mut likely_languages = Vec::new();

        let mut entries = fs::read_dir(workspace)
            .await
            .context("Failed to read workspace directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();

            // Ignore hidden files and common build directories
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }

            top_level_files.push(name.clone());

            // Simple language heuristics based on top-level markers
            if name == "Cargo.toml" {
                likely_languages.push("Rust".to_string());
            } else if name == "package.json" {
                likely_languages.push("JavaScript/TypeScript".to_string());
            } else if name == "go.mod" {
                likely_languages.push("Go".to_string());
            } else if name == "requirements.txt" || name == "pyproject.toml" {
                likely_languages.push("Python".to_string());
            }
        }

        top_level_files.sort();
        likely_languages.sort();
        likely_languages.dedup();

        Ok(Self {
            workspace_path: workspace.to_path_buf(),
            top_level_files,
            likely_languages,
        })
    }

    /// Format this project memory into a system prompt injection string
    pub fn format_for_prompt(&self) -> String {
        let files = self.top_level_files.join(", ");
        let languages = if self.likely_languages.is_empty() {
            "Unknown".to_string()
        } else {
            self.likely_languages.join(", ")
        };

        format!(
            "Workspace: {}\nLanguages: {}\nFiles: {}",
            self.workspace_path.display(),
            languages,
            files
        )
    }
}
