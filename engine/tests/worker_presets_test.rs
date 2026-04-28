//! Tests for system::worker_presets — preset definitions, execution profiles, subagent specs

use rove_engine::system::worker_presets::{
    execution_profile_for_preset, list_worker_presets, subagent_spec_for_preset, worker_preset,
};
use sdk::SubagentRole;

// ── list_worker_presets ────────────────────────────────────────────────────────

#[test]
fn list_worker_presets_returns_four() {
    let presets = list_worker_presets();
    assert_eq!(presets.len(), 4);
}

#[test]
fn list_worker_presets_has_researcher() {
    let presets = list_worker_presets();
    assert!(presets.iter().any(|p| p.id == "researcher"));
}

#[test]
fn list_worker_presets_has_executor() {
    let presets = list_worker_presets();
    assert!(presets.iter().any(|p| p.id == "executor"));
}

#[test]
fn list_worker_presets_has_verifier() {
    let presets = list_worker_presets();
    assert!(presets.iter().any(|p| p.id == "verifier"));
}

#[test]
fn list_worker_presets_has_summariser() {
    let presets = list_worker_presets();
    assert!(presets.iter().any(|p| p.id == "summariser"));
}

#[test]
fn all_presets_have_nonempty_id() {
    for preset in list_worker_presets() {
        assert!(!preset.id.is_empty(), "Preset id is empty");
    }
}

#[test]
fn all_presets_have_nonempty_name() {
    for preset in list_worker_presets() {
        assert!(!preset.name.is_empty(), "Preset name is empty for {}", preset.id);
    }
}

#[test]
fn all_presets_have_nonempty_description() {
    for preset in list_worker_presets() {
        assert!(!preset.description.is_empty(), "Description empty for {}", preset.id);
    }
}

#[test]
fn all_presets_have_nonempty_instructions() {
    for preset in list_worker_presets() {
        assert!(!preset.instructions.is_empty(), "Instructions empty for {}", preset.id);
    }
}

#[test]
fn all_presets_have_allowed_tools() {
    for preset in list_worker_presets() {
        assert!(!preset.allowed_tools.is_empty(), "No allowed tools for {}", preset.id);
    }
}

#[test]
fn all_presets_have_positive_timeout() {
    for preset in list_worker_presets() {
        assert!(preset.timeout_secs > 0, "timeout=0 for {}", preset.id);
    }
}

#[test]
fn all_presets_have_positive_max_steps() {
    for preset in list_worker_presets() {
        assert!(preset.max_steps > 0, "max_steps=0 for {}", preset.id);
    }
}

#[test]
fn all_presets_have_positive_memory_budget() {
    for preset in list_worker_presets() {
        assert!(preset.memory_budget > 0, "memory_budget=0 for {}", preset.id);
    }
}

// ── Researcher preset specifics ────────────────────────────────────────────────

#[test]
fn researcher_role_is_researcher() {
    let preset = worker_preset("researcher").unwrap();
    assert_eq!(preset.role, "researcher");
}

#[test]
fn researcher_has_read_file_tool() {
    let preset = worker_preset("researcher").unwrap();
    assert!(preset.allowed_tools.contains(&"read_file".to_string()));
}

#[test]
fn researcher_has_list_dir_tool() {
    let preset = worker_preset("researcher").unwrap();
    assert!(preset.allowed_tools.contains(&"list_dir".to_string()));
}

#[test]
fn researcher_has_run_command_tool() {
    let preset = worker_preset("researcher").unwrap();
    assert!(preset.allowed_tools.contains(&"run_command".to_string()));
}

#[test]
fn researcher_does_not_have_write_file_tool() {
    let preset = worker_preset("researcher").unwrap();
    assert!(!preset.allowed_tools.contains(&"write_file".to_string()));
}

#[test]
fn researcher_max_iterations_is_4() {
    let preset = worker_preset("researcher").unwrap();
    assert_eq!(preset.max_iterations, Some(4));
}

#[test]
fn researcher_max_steps_is_6() {
    let preset = worker_preset("researcher").unwrap();
    assert_eq!(preset.max_steps, 6);
}

#[test]
fn researcher_timeout_is_90() {
    let preset = worker_preset("researcher").unwrap();
    assert_eq!(preset.timeout_secs, 90);
}

#[test]
fn researcher_has_output_contract() {
    let preset = worker_preset("researcher").unwrap();
    assert!(preset.output_contract.is_some());
}

// ── Executor preset specifics ──────────────────────────────────────────────────

#[test]
fn executor_role_is_executor() {
    let preset = worker_preset("executor").unwrap();
    assert_eq!(preset.role, "executor");
}

#[test]
fn executor_has_write_file_tool() {
    let preset = worker_preset("executor").unwrap();
    assert!(preset.allowed_tools.contains(&"write_file".to_string()));
}

#[test]
fn executor_has_read_file_tool() {
    let preset = worker_preset("executor").unwrap();
    assert!(preset.allowed_tools.contains(&"read_file".to_string()));
}

#[test]
fn executor_max_iterations_is_6() {
    let preset = worker_preset("executor").unwrap();
    assert_eq!(preset.max_iterations, Some(6));
}

#[test]
fn executor_max_steps_is_8() {
    let preset = worker_preset("executor").unwrap();
    assert_eq!(preset.max_steps, 8);
}

#[test]
fn executor_timeout_is_120() {
    let preset = worker_preset("executor").unwrap();
    assert_eq!(preset.timeout_secs, 120);
}

#[test]
fn executor_has_output_contract() {
    let preset = worker_preset("executor").unwrap();
    assert!(preset.output_contract.is_some());
}

// ── Verifier preset specifics ──────────────────────────────────────────────────

#[test]
fn verifier_role_is_verifier() {
    let preset = worker_preset("verifier").unwrap();
    assert_eq!(preset.role, "verifier");
}

#[test]
fn verifier_does_not_have_write_file_tool() {
    let preset = worker_preset("verifier").unwrap();
    assert!(!preset.allowed_tools.contains(&"write_file".to_string()));
}

#[test]
fn verifier_max_iterations_is_4() {
    let preset = worker_preset("verifier").unwrap();
    assert_eq!(preset.max_iterations, Some(4));
}

#[test]
fn verifier_timeout_is_90() {
    let preset = worker_preset("verifier").unwrap();
    assert_eq!(preset.timeout_secs, 90);
}

#[test]
fn verifier_max_steps_is_6() {
    let preset = worker_preset("verifier").unwrap();
    assert_eq!(preset.max_steps, 6);
}

// ── Summariser preset specifics ────────────────────────────────────────────────

#[test]
fn summariser_role_is_summariser() {
    let preset = worker_preset("summariser").unwrap();
    assert_eq!(preset.role, "summariser");
}

#[test]
fn summariser_does_not_have_write_file() {
    let preset = worker_preset("summariser").unwrap();
    assert!(!preset.allowed_tools.contains(&"write_file".to_string()));
}

#[test]
fn summariser_max_iterations_is_3() {
    let preset = worker_preset("summariser").unwrap();
    assert_eq!(preset.max_iterations, Some(3));
}

#[test]
fn summariser_timeout_is_60() {
    let preset = worker_preset("summariser").unwrap();
    assert_eq!(preset.timeout_secs, 60);
}

#[test]
fn summariser_max_steps_is_4() {
    let preset = worker_preset("summariser").unwrap();
    assert_eq!(preset.max_steps, 4);
}

// ── worker_preset() error cases ────────────────────────────────────────────────

#[test]
fn worker_preset_unknown_id_returns_error() {
    let result = worker_preset("nonexistent-preset");
    assert!(result.is_err());
}

#[test]
fn worker_preset_empty_id_returns_error() {
    let result = worker_preset("");
    assert!(result.is_err());
}

#[test]
fn worker_preset_close_name_returns_error() {
    let result = worker_preset("researche");
    assert!(result.is_err());
}

// ── execution_profile_for_preset ──────────────────────────────────────────────

#[test]
fn profile_researcher_id_correct() {
    let profile = execution_profile_for_preset("researcher").unwrap();
    assert_eq!(profile.worker_preset_id.as_deref(), Some("researcher"));
}

#[test]
fn profile_researcher_name_correct() {
    let profile = execution_profile_for_preset("researcher").unwrap();
    assert_eq!(profile.worker_preset_name.as_deref(), Some("Researcher"));
}

#[test]
fn profile_researcher_max_iterations() {
    let profile = execution_profile_for_preset("researcher").unwrap();
    assert_eq!(profile.max_iterations, Some(4));
}

#[test]
fn profile_executor_id_correct() {
    let profile = execution_profile_for_preset("executor").unwrap();
    assert_eq!(profile.worker_preset_id.as_deref(), Some("executor"));
}

#[test]
fn profile_executor_max_iterations() {
    let profile = execution_profile_for_preset("executor").unwrap();
    assert_eq!(profile.max_iterations, Some(6));
}

#[test]
fn profile_verifier_max_iterations() {
    let profile = execution_profile_for_preset("verifier").unwrap();
    assert_eq!(profile.max_iterations, Some(4));
}

#[test]
fn profile_researcher_has_run_command() {
    let profile = execution_profile_for_preset("researcher").unwrap();
    assert!(profile.allowed_tools.iter().any(|t| t == "run_command"));
}

#[test]
fn profile_callable_agents_empty() {
    let profile = execution_profile_for_preset("executor").unwrap();
    assert!(profile.callable_agents.is_empty());
}

#[test]
fn profile_unknown_returns_error() {
    assert!(execution_profile_for_preset("unknown-preset").is_err());
}

#[test]
fn profile_has_instructions() {
    let profile = execution_profile_for_preset("executor").unwrap();
    assert!(!profile.instructions.is_empty());
}

#[test]
fn profile_has_purpose() {
    let profile = execution_profile_for_preset("executor").unwrap();
    assert!(profile.purpose.is_some());
    assert!(!profile.purpose.unwrap().is_empty());
}

#[test]
fn profile_output_contract_present() {
    let profile = execution_profile_for_preset("researcher").unwrap();
    assert!(profile.output_contract.is_some());
}

// ── subagent_spec_for_preset ───────────────────────────────────────────────────

#[test]
fn subagent_spec_researcher_role() {
    let spec = subagent_spec_for_preset("researcher", "investigate bug", vec![]).unwrap();
    assert_eq!(spec.role, SubagentRole::Researcher);
}

#[test]
fn subagent_spec_executor_role() {
    let spec = subagent_spec_for_preset("executor", "apply fix", vec![]).unwrap();
    assert_eq!(spec.role, SubagentRole::Executor);
}

#[test]
fn subagent_spec_verifier_role() {
    let spec = subagent_spec_for_preset("verifier", "verify fix", vec![]).unwrap();
    assert_eq!(spec.role, SubagentRole::Verifier);
}

#[test]
fn subagent_spec_summariser_role() {
    let spec = subagent_spec_for_preset("summariser", "summarise", vec![]).unwrap();
    assert_eq!(spec.role, SubagentRole::Summariser);
}

#[test]
fn subagent_spec_task_stored() {
    let spec = subagent_spec_for_preset("researcher", "my task here", vec![]).unwrap();
    assert_eq!(spec.task, "my task here");
}

#[test]
fn subagent_spec_empty_allowed_tools_uses_preset_tools() {
    let spec = subagent_spec_for_preset("researcher", "task", vec![]).unwrap();
    assert!(!spec.tools_allowed.is_empty());
}

#[test]
fn subagent_spec_filters_to_allowed_tools() {
    let spec = subagent_spec_for_preset(
        "summariser",
        "summarise",
        vec!["read_file".to_string(), "write_file".to_string()],
    )
    .unwrap();
    // summariser allows read_file but not write_file
    assert!(spec.tools_allowed.contains(&"read_file".to_string()));
    assert!(!spec.tools_allowed.contains(&"write_file".to_string()));
}

#[test]
fn subagent_spec_all_filtered_tools_not_in_preset_gives_empty() {
    let spec = subagent_spec_for_preset(
        "researcher",
        "task",
        vec!["write_file".to_string()], // researcher doesn't allow write_file
    )
    .unwrap();
    assert!(!spec.tools_allowed.contains(&"write_file".to_string()));
}

#[test]
fn subagent_spec_memory_budget_from_preset() {
    let preset = worker_preset("researcher").unwrap();
    let spec = subagent_spec_for_preset("researcher", "task", vec![]).unwrap();
    assert_eq!(spec.memory_budget, preset.memory_budget);
}

#[test]
fn subagent_spec_max_steps_from_preset() {
    let preset = worker_preset("executor").unwrap();
    let spec = subagent_spec_for_preset("executor", "task", vec![]).unwrap();
    assert_eq!(spec.max_steps, preset.max_steps);
}

#[test]
fn subagent_spec_timeout_from_preset() {
    let preset = worker_preset("verifier").unwrap();
    let spec = subagent_spec_for_preset("verifier", "task", vec![]).unwrap();
    assert_eq!(spec.timeout_secs, preset.timeout_secs);
}

#[test]
fn subagent_spec_no_model_override() {
    let spec = subagent_spec_for_preset("executor", "task", vec![]).unwrap();
    assert!(spec.model_override.is_none());
}

#[test]
fn subagent_spec_unknown_preset_returns_error() {
    assert!(subagent_spec_for_preset("bogus", "task", vec![]).is_err());
}
