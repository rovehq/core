//! Extended tests for system::worker_presets — detailed preset field values

use rove_engine::system::worker_presets::{
    execution_profile_for_preset, list_worker_presets, subagent_spec_for_preset, worker_preset,
};
use sdk::SubagentRole;

// ── list_worker_presets ───────────────────────────────────────────────────────

#[test]
fn list_returns_four_presets() {
    assert_eq!(list_worker_presets().len(), 4);
}

#[test]
fn list_contains_researcher() {
    let presets = list_worker_presets();
    assert!(presets.iter().any(|p| p.id == "researcher"));
}

#[test]
fn list_contains_executor() {
    let presets = list_worker_presets();
    assert!(presets.iter().any(|p| p.id == "executor"));
}

#[test]
fn list_contains_verifier() {
    let presets = list_worker_presets();
    assert!(presets.iter().any(|p| p.id == "verifier"));
}

#[test]
fn list_contains_summariser() {
    let presets = list_worker_presets();
    assert!(presets.iter().any(|p| p.id == "summariser"));
}

#[test]
fn all_presets_have_nonempty_name() {
    for p in list_worker_presets() {
        assert!(!p.name.is_empty(), "empty name for id={}", p.id);
    }
}

#[test]
fn all_presets_have_nonempty_description() {
    for p in list_worker_presets() {
        assert!(!p.description.is_empty(), "empty description for id={}", p.id);
    }
}

#[test]
fn all_presets_have_nonempty_instructions() {
    for p in list_worker_presets() {
        assert!(!p.instructions.is_empty(), "empty instructions for id={}", p.id);
    }
}

#[test]
fn all_presets_have_at_least_one_tool() {
    for p in list_worker_presets() {
        assert!(!p.allowed_tools.is_empty(), "no tools for id={}", p.id);
    }
}

#[test]
fn all_presets_have_output_contract() {
    for p in list_worker_presets() {
        assert!(p.output_contract.is_some(), "no output_contract for id={}", p.id);
    }
}

#[test]
fn all_presets_have_max_iterations() {
    for p in list_worker_presets() {
        assert!(p.max_iterations.is_some(), "no max_iterations for id={}", p.id);
    }
}

#[test]
fn all_presets_positive_max_steps() {
    for p in list_worker_presets() {
        assert!(p.max_steps > 0, "max_steps=0 for id={}", p.id);
    }
}

#[test]
fn all_presets_positive_timeout_secs() {
    for p in list_worker_presets() {
        assert!(p.timeout_secs > 0, "timeout_secs=0 for id={}", p.id);
    }
}

#[test]
fn all_presets_positive_memory_budget() {
    for p in list_worker_presets() {
        assert!(p.memory_budget > 0, "memory_budget=0 for id={}", p.id);
    }
}

// ── researcher preset ─────────────────────────────────────────────────────────

#[test]
fn researcher_has_read_file_tool() {
    let p = worker_preset("researcher").unwrap();
    assert!(p.allowed_tools.contains(&"read_file".to_string()));
}

#[test]
fn researcher_has_list_dir_tool() {
    let p = worker_preset("researcher").unwrap();
    assert!(p.allowed_tools.contains(&"list_dir".to_string()));
}

#[test]
fn researcher_has_run_command_tool() {
    let p = worker_preset("researcher").unwrap();
    assert!(p.allowed_tools.contains(&"run_command".to_string()));
}

#[test]
fn researcher_has_no_write_file() {
    let p = worker_preset("researcher").unwrap();
    assert!(!p.allowed_tools.contains(&"write_file".to_string()));
}

#[test]
fn researcher_max_iterations_is_4() {
    let p = worker_preset("researcher").unwrap();
    assert_eq!(p.max_iterations, Some(4));
}

#[test]
fn researcher_max_steps_is_6() {
    let p = worker_preset("researcher").unwrap();
    assert_eq!(p.max_steps, 6);
}

#[test]
fn researcher_timeout_is_90() {
    let p = worker_preset("researcher").unwrap();
    assert_eq!(p.timeout_secs, 90);
}

#[test]
fn researcher_memory_budget_is_1200() {
    let p = worker_preset("researcher").unwrap();
    assert_eq!(p.memory_budget, 1200);
}

// ── executor preset ───────────────────────────────────────────────────────────

#[test]
fn executor_has_write_file_tool() {
    let p = worker_preset("executor").unwrap();
    assert!(p.allowed_tools.contains(&"write_file".to_string()));
}

#[test]
fn executor_has_read_file_tool() {
    let p = worker_preset("executor").unwrap();
    assert!(p.allowed_tools.contains(&"read_file".to_string()));
}

#[test]
fn executor_has_run_command_tool() {
    let p = worker_preset("executor").unwrap();
    assert!(p.allowed_tools.contains(&"run_command".to_string()));
}

#[test]
fn executor_max_iterations_is_6() {
    let p = worker_preset("executor").unwrap();
    assert_eq!(p.max_iterations, Some(6));
}

#[test]
fn executor_max_steps_is_8() {
    let p = worker_preset("executor").unwrap();
    assert_eq!(p.max_steps, 8);
}

#[test]
fn executor_timeout_is_120() {
    let p = worker_preset("executor").unwrap();
    assert_eq!(p.timeout_secs, 120);
}

#[test]
fn executor_memory_budget_is_900() {
    let p = worker_preset("executor").unwrap();
    assert_eq!(p.memory_budget, 900);
}

// ── verifier preset ───────────────────────────────────────────────────────────

#[test]
fn verifier_has_run_command_tool() {
    let p = worker_preset("verifier").unwrap();
    assert!(p.allowed_tools.contains(&"run_command".to_string()));
}

#[test]
fn verifier_has_no_write_file() {
    let p = worker_preset("verifier").unwrap();
    assert!(!p.allowed_tools.contains(&"write_file".to_string()));
}

#[test]
fn verifier_max_iterations_is_4() {
    let p = worker_preset("verifier").unwrap();
    assert_eq!(p.max_iterations, Some(4));
}

#[test]
fn verifier_max_steps_is_6() {
    let p = worker_preset("verifier").unwrap();
    assert_eq!(p.max_steps, 6);
}

#[test]
fn verifier_timeout_is_90() {
    let p = worker_preset("verifier").unwrap();
    assert_eq!(p.timeout_secs, 90);
}

#[test]
fn verifier_memory_budget_is_800() {
    let p = worker_preset("verifier").unwrap();
    assert_eq!(p.memory_budget, 800);
}

// ── summariser preset ─────────────────────────────────────────────────────────

#[test]
fn summariser_has_read_file_tool() {
    let p = worker_preset("summariser").unwrap();
    assert!(p.allowed_tools.contains(&"read_file".to_string()));
}

#[test]
fn summariser_has_no_run_command() {
    let p = worker_preset("summariser").unwrap();
    assert!(!p.allowed_tools.contains(&"run_command".to_string()));
}

#[test]
fn summariser_max_iterations_is_3() {
    let p = worker_preset("summariser").unwrap();
    assert_eq!(p.max_iterations, Some(3));
}

#[test]
fn summariser_max_steps_is_4() {
    let p = worker_preset("summariser").unwrap();
    assert_eq!(p.max_steps, 4);
}

#[test]
fn summariser_timeout_is_60() {
    let p = worker_preset("summariser").unwrap();
    assert_eq!(p.timeout_secs, 60);
}

#[test]
fn summariser_memory_budget_is_600() {
    let p = worker_preset("summariser").unwrap();
    assert_eq!(p.memory_budget, 600);
}

// ── worker_preset: unknown ────────────────────────────────────────────────────

#[test]
fn unknown_preset_returns_error() {
    assert!(worker_preset("unknown").is_err());
}

#[test]
fn empty_preset_id_returns_error() {
    assert!(worker_preset("").is_err());
}

#[test]
fn typo_preset_returns_error() {
    assert!(worker_preset("reseacher").is_err()); // typo
}

// ── execution_profile_for_preset ─────────────────────────────────────────────

#[test]
fn profile_researcher_preset_id() {
    let p = execution_profile_for_preset("researcher").unwrap();
    assert_eq!(p.worker_preset_id.as_deref(), Some("researcher"));
}

#[test]
fn profile_executor_preset_id() {
    let p = execution_profile_for_preset("executor").unwrap();
    assert_eq!(p.worker_preset_id.as_deref(), Some("executor"));
}

#[test]
fn profile_verifier_preset_id() {
    let p = execution_profile_for_preset("verifier").unwrap();
    assert_eq!(p.worker_preset_id.as_deref(), Some("verifier"));
}

#[test]
fn profile_summariser_preset_id() {
    let p = execution_profile_for_preset("summariser").unwrap();
    assert_eq!(p.worker_preset_id.as_deref(), Some("summariser"));
}

#[test]
fn profile_unknown_returns_error() {
    assert!(execution_profile_for_preset("unknown").is_err());
}

#[test]
fn profile_researcher_has_max_iterations() {
    let p = execution_profile_for_preset("researcher").unwrap();
    assert_eq!(p.max_iterations, Some(4));
}

#[test]
fn profile_executor_has_max_iterations_6() {
    let p = execution_profile_for_preset("executor").unwrap();
    assert_eq!(p.max_iterations, Some(6));
}

#[test]
fn profile_has_allowed_tools() {
    let p = execution_profile_for_preset("researcher").unwrap();
    assert!(!p.allowed_tools.is_empty());
}

#[test]
fn profile_has_instructions() {
    let p = execution_profile_for_preset("executor").unwrap();
    assert!(!p.instructions.is_empty());
}

#[test]
fn profile_agent_id_is_none() {
    let p = execution_profile_for_preset("verifier").unwrap();
    assert!(p.agent_id.is_none());
}

#[test]
fn profile_callable_agents_empty() {
    let p = execution_profile_for_preset("summariser").unwrap();
    assert!(p.callable_agents.is_empty());
}

// ── subagent_spec_for_preset ──────────────────────────────────────────────────

#[test]
fn subagent_researcher_role() {
    let spec = subagent_spec_for_preset("researcher", "investigate", vec![]).unwrap();
    assert_eq!(spec.role, SubagentRole::Researcher);
}

#[test]
fn subagent_executor_role() {
    let spec = subagent_spec_for_preset("executor", "apply fix", vec![]).unwrap();
    assert_eq!(spec.role, SubagentRole::Executor);
}

#[test]
fn subagent_verifier_role() {
    let spec = subagent_spec_for_preset("verifier", "verify result", vec![]).unwrap();
    assert_eq!(spec.role, SubagentRole::Verifier);
}

#[test]
fn subagent_summariser_role() {
    let spec = subagent_spec_for_preset("summariser", "summarize", vec![]).unwrap();
    assert_eq!(spec.role, SubagentRole::Summariser);
}

#[test]
fn subagent_empty_allowed_tools_uses_preset_default() {
    let spec = subagent_spec_for_preset("researcher", "task", vec![]).unwrap();
    // Empty override → uses preset's tools
    assert!(!spec.tools_allowed.is_empty());
}

#[test]
fn subagent_filter_keeps_allowed_intersection() {
    let spec = subagent_spec_for_preset(
        "executor",
        "fix bug",
        vec!["read_file".to_string(), "write_file".to_string()],
    ).unwrap();
    assert!(spec.tools_allowed.contains(&"read_file".to_string()));
    assert!(spec.tools_allowed.contains(&"write_file".to_string()));
}

#[test]
fn subagent_filter_removes_tools_not_in_preset() {
    let spec = subagent_spec_for_preset(
        "summariser",
        "summarise",
        vec!["read_file".to_string(), "write_file".to_string()],
    ).unwrap();
    // summariser doesn't have write_file
    assert!(!spec.tools_allowed.contains(&"write_file".to_string()));
    assert!(spec.tools_allowed.contains(&"read_file".to_string()));
}

#[test]
fn subagent_unknown_preset_returns_error() {
    assert!(subagent_spec_for_preset("unknown", "task", vec![]).is_err());
}

#[test]
fn subagent_researcher_memory_budget_is_1200() {
    let spec = subagent_spec_for_preset("researcher", "task", vec![]).unwrap();
    assert_eq!(spec.memory_budget, 1200);
}

#[test]
fn subagent_summariser_memory_budget_is_600() {
    let spec = subagent_spec_for_preset("summariser", "task", vec![]).unwrap();
    assert_eq!(spec.memory_budget, 600);
}

#[test]
fn subagent_researcher_max_steps_is_6() {
    let spec = subagent_spec_for_preset("researcher", "task", vec![]).unwrap();
    assert_eq!(spec.max_steps, 6);
}

#[test]
fn subagent_executor_max_steps_is_8() {
    let spec = subagent_spec_for_preset("executor", "task", vec![]).unwrap();
    assert_eq!(spec.max_steps, 8);
}

#[test]
fn subagent_researcher_timeout_is_90() {
    let spec = subagent_spec_for_preset("researcher", "task", vec![]).unwrap();
    assert_eq!(spec.timeout_secs, 90);
}

#[test]
fn subagent_executor_timeout_is_120() {
    let spec = subagent_spec_for_preset("executor", "task", vec![]).unwrap();
    assert_eq!(spec.timeout_secs, 120);
}

#[test]
fn subagent_task_stored_correctly() {
    let task = "investigate the bug in module X";
    let spec = subagent_spec_for_preset("researcher", task, vec![]).unwrap();
    assert_eq!(spec.task, task);
}

#[test]
fn subagent_model_override_is_none() {
    let spec = subagent_spec_for_preset("verifier", "task", vec![]).unwrap();
    assert!(spec.model_override.is_none());
}
