//! Tests for system::workflow_triggers — default_channel_targets, normalize_watch_event,
//! WorkflowTriggerMatch, FileWatchRegistration struct fields

use rove_engine::system::workflow_triggers::{
    default_channel_targets, normalize_watch_event, FileWatchRegistration, WorkflowTriggerMatch,
};
use std::path::PathBuf;

// ── default_channel_targets ───────────────────────────────────────────────────

#[test]
fn default_targets_always_has_default() {
    let targets = default_channel_targets(None);
    assert!(targets.contains(&"default".to_string()));
}

#[test]
fn default_targets_none_is_single_element() {
    let targets = default_channel_targets(None);
    assert_eq!(targets.len(), 1);
}

#[test]
fn default_targets_empty_str_same_as_none() {
    let targets = default_channel_targets(Some(""));
    assert_eq!(targets, vec!["default".to_string()]);
}

#[test]
fn default_targets_whitespace_same_as_none() {
    let targets = default_channel_targets(Some("   "));
    assert_eq!(targets, vec!["default".to_string()]);
}

#[test]
fn default_targets_new_value_appended() {
    let targets = default_channel_targets(Some("chat:123"));
    assert_eq!(
        targets,
        vec!["default".to_string(), "chat:123".to_string()]
    );
}

#[test]
fn default_targets_duplicate_default_not_added() {
    let targets = default_channel_targets(Some("default"));
    assert_eq!(targets, vec!["default".to_string()]);
    assert_eq!(targets.len(), 1);
}

#[test]
fn default_targets_duplicate_default_case_insensitive() {
    let targets = default_channel_targets(Some("DEFAULT"));
    assert_eq!(targets.len(), 1);
}

#[test]
fn default_targets_case_insensitive_dedup_mixed() {
    let targets = default_channel_targets(Some("Default"));
    assert_eq!(targets.len(), 1);
}

#[test]
fn default_targets_extra_value_trimmed() {
    let targets = default_channel_targets(Some("  chat:456  "));
    assert_eq!(targets[1], "chat:456");
}

#[test]
fn default_targets_first_element_is_always_default() {
    let targets = default_channel_targets(Some("telegram:1234"));
    assert_eq!(targets[0], "default");
}

#[test]
fn default_targets_with_telegram_id() {
    let targets = default_channel_targets(Some("telegram:999"));
    assert!(targets.contains(&"telegram:999".to_string()));
}

#[test]
fn default_targets_length_two_with_extra() {
    let targets = default_channel_targets(Some("custom"));
    assert_eq!(targets.len(), 2);
}

#[test]
fn default_targets_special_chars_in_extra() {
    let targets = default_channel_targets(Some("group:abc-123_xyz"));
    assert!(targets.contains(&"group:abc-123_xyz".to_string()));
}

#[test]
fn default_targets_numeric_channel() {
    let targets = default_channel_targets(Some("42"));
    assert!(targets.contains(&"42".to_string()));
}

// ── normalize_watch_event ─────────────────────────────────────────────────────

#[test]
fn normalize_create_event() {
    assert_eq!(normalize_watch_event("create"), "create");
}

#[test]
fn normalize_modify_event() {
    assert_eq!(normalize_watch_event("modify"), "modify");
}

#[test]
fn normalize_remove_event() {
    assert_eq!(normalize_watch_event("remove"), "remove");
}

#[test]
fn normalize_unknown_event_returns_any() {
    assert_eq!(normalize_watch_event("unknown"), "any");
}

#[test]
fn normalize_empty_event_returns_any() {
    assert_eq!(normalize_watch_event(""), "any");
}

#[test]
fn normalize_create_uppercase() {
    assert_eq!(normalize_watch_event("CREATE"), "create");
}

#[test]
fn normalize_modify_uppercase() {
    assert_eq!(normalize_watch_event("MODIFY"), "modify");
}

#[test]
fn normalize_remove_uppercase() {
    assert_eq!(normalize_watch_event("REMOVE"), "remove");
}

#[test]
fn normalize_mixed_case_create() {
    assert_eq!(normalize_watch_event("Create"), "create");
}

#[test]
fn normalize_mixed_case_modify() {
    assert_eq!(normalize_watch_event("Modify"), "modify");
}

#[test]
fn normalize_mixed_case_remove() {
    assert_eq!(normalize_watch_event("Remove"), "remove");
}

#[test]
fn normalize_whitespace_trimmed_create() {
    assert_eq!(normalize_watch_event("  create  "), "create");
}

#[test]
fn normalize_whitespace_trimmed_modify() {
    assert_eq!(normalize_watch_event("  modify  "), "modify");
}

#[test]
fn normalize_whitespace_trimmed_remove() {
    assert_eq!(normalize_watch_event("  remove  "), "remove");
}

#[test]
fn normalize_delete_is_any() {
    assert_eq!(normalize_watch_event("delete"), "any");
}

#[test]
fn normalize_write_is_any() {
    assert_eq!(normalize_watch_event("write"), "any");
}

#[test]
fn normalize_change_is_any() {
    assert_eq!(normalize_watch_event("change"), "any");
}

#[test]
fn normalize_all_event_is_any() {
    assert_eq!(normalize_watch_event("all"), "any");
}

#[test]
fn normalize_numeric_is_any() {
    assert_eq!(normalize_watch_event("123"), "any");
}

#[test]
fn normalize_random_string_is_any() {
    assert_eq!(normalize_watch_event("inotify"), "any");
}

// ── WorkflowTriggerMatch struct ───────────────────────────────────────────────

#[test]
fn trigger_match_fields_accessible() {
    let m = WorkflowTriggerMatch {
        workflow_id: "wf-1".to_string(),
        workflow_name: "My Workflow".to_string(),
        binding_target: Some("chat:123".to_string()),
    };
    assert_eq!(m.workflow_id, "wf-1");
    assert_eq!(m.workflow_name, "My Workflow");
    assert_eq!(m.binding_target, Some("chat:123".to_string()));
}

#[test]
fn trigger_match_no_binding_target() {
    let m = WorkflowTriggerMatch {
        workflow_id: "wf-2".to_string(),
        workflow_name: "Other".to_string(),
        binding_target: None,
    };
    assert!(m.binding_target.is_none());
}

#[test]
fn trigger_match_clone() {
    let m = WorkflowTriggerMatch {
        workflow_id: "wf-3".to_string(),
        workflow_name: "Cloned".to_string(),
        binding_target: Some("target".to_string()),
    };
    let m2 = m.clone();
    assert_eq!(m, m2);
}

#[test]
fn trigger_match_equality() {
    let a = WorkflowTriggerMatch {
        workflow_id: "wf-4".to_string(),
        workflow_name: "A".to_string(),
        binding_target: None,
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn trigger_match_inequality_different_id() {
    let a = WorkflowTriggerMatch {
        workflow_id: "wf-4".to_string(),
        workflow_name: "A".to_string(),
        binding_target: None,
    };
    let b = WorkflowTriggerMatch {
        workflow_id: "wf-5".to_string(),
        workflow_name: "A".to_string(),
        binding_target: None,
    };
    assert_ne!(a, b);
}

#[test]
fn trigger_match_debug_format() {
    let m = WorkflowTriggerMatch {
        workflow_id: "wf-6".to_string(),
        workflow_name: "Debug".to_string(),
        binding_target: None,
    };
    let s = format!("{:?}", m);
    assert!(s.contains("wf-6"));
}

#[test]
fn trigger_match_serializes() {
    let m = WorkflowTriggerMatch {
        workflow_id: "wf-7".to_string(),
        workflow_name: "Serialized".to_string(),
        binding_target: Some("t".to_string()),
    };
    let j = serde_json::to_string(&m).unwrap();
    assert!(j.contains("wf-7"));
    assert!(j.contains("Serialized"));
}

#[test]
fn trigger_match_serializes_null_target() {
    let m = WorkflowTriggerMatch {
        workflow_id: "wf-8".to_string(),
        workflow_name: "NoTarget".to_string(),
        binding_target: None,
    };
    let j = serde_json::to_string(&m).unwrap();
    assert!(j.contains("null") || j.contains("wf-8"));
}

// ── FileWatchRegistration struct ──────────────────────────────────────────────

#[test]
fn file_watch_fields_accessible() {
    let reg = FileWatchRegistration {
        workflow_id: "wf-watch".to_string(),
        workflow_name: "Watch Workflow".to_string(),
        path: PathBuf::from("/workspace/src"),
        recursive: true,
    };
    assert_eq!(reg.workflow_id, "wf-watch");
    assert_eq!(reg.workflow_name, "Watch Workflow");
    assert_eq!(reg.path, PathBuf::from("/workspace/src"));
    assert!(reg.recursive);
}

#[test]
fn file_watch_non_recursive() {
    let reg = FileWatchRegistration {
        workflow_id: "wf-flat".to_string(),
        workflow_name: "Flat".to_string(),
        path: PathBuf::from("/workspace/docs"),
        recursive: false,
    };
    assert!(!reg.recursive);
}

#[test]
fn file_watch_clone() {
    let reg = FileWatchRegistration {
        workflow_id: "wf-c".to_string(),
        workflow_name: "Cloned".to_string(),
        path: PathBuf::from("/tmp/watch"),
        recursive: true,
    };
    let reg2 = reg.clone();
    assert_eq!(reg, reg2);
}

#[test]
fn file_watch_equality() {
    let a = FileWatchRegistration {
        workflow_id: "wf-eq".to_string(),
        workflow_name: "Eq".to_string(),
        path: PathBuf::from("/tmp/eq"),
        recursive: false,
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn file_watch_inequality_different_path() {
    let a = FileWatchRegistration {
        workflow_id: "wf-neq".to_string(),
        workflow_name: "Neq".to_string(),
        path: PathBuf::from("/tmp/a"),
        recursive: false,
    };
    let b = FileWatchRegistration {
        workflow_id: "wf-neq".to_string(),
        workflow_name: "Neq".to_string(),
        path: PathBuf::from("/tmp/b"),
        recursive: false,
    };
    assert_ne!(a, b);
}

#[test]
fn file_watch_debug_format() {
    let reg = FileWatchRegistration {
        workflow_id: "wf-d".to_string(),
        workflow_name: "Debug".to_string(),
        path: PathBuf::from("/debug"),
        recursive: false,
    };
    let s = format!("{:?}", reg);
    assert!(s.contains("wf-d"));
}

#[test]
fn file_watch_path_relative() {
    let reg = FileWatchRegistration {
        workflow_id: "wf-rel".to_string(),
        workflow_name: "Relative".to_string(),
        path: PathBuf::from("src/lib.rs"),
        recursive: false,
    };
    assert!(!reg.path.is_absolute());
}

#[test]
fn file_watch_path_absolute() {
    let reg = FileWatchRegistration {
        workflow_id: "wf-abs".to_string(),
        workflow_name: "Absolute".to_string(),
        path: PathBuf::from("/absolute/path"),
        recursive: true,
    };
    assert!(reg.path.is_absolute());
}

// ── Sorting / dedup sanity (pure struct tests) ────────────────────────────────

#[test]
fn trigger_matches_can_be_sorted_by_name() {
    let mut matches = vec![
        WorkflowTriggerMatch {
            workflow_id: "b".to_string(),
            workflow_name: "B Workflow".to_string(),
            binding_target: None,
        },
        WorkflowTriggerMatch {
            workflow_id: "a".to_string(),
            workflow_name: "A Workflow".to_string(),
            binding_target: None,
        },
    ];
    matches.sort_by(|l, r| l.workflow_name.cmp(&r.workflow_name));
    assert_eq!(matches[0].workflow_id, "a");
}

#[test]
fn file_watch_registrations_can_be_sorted_by_path() {
    let mut regs = vec![
        FileWatchRegistration {
            workflow_id: "b".to_string(),
            workflow_name: "B".to_string(),
            path: PathBuf::from("/z/path"),
            recursive: false,
        },
        FileWatchRegistration {
            workflow_id: "a".to_string(),
            workflow_name: "A".to_string(),
            path: PathBuf::from("/a/path"),
            recursive: false,
        },
    ];
    regs.sort_by(|l, r| l.path.cmp(&r.path));
    assert_eq!(regs[0].workflow_id, "a");
}

#[test]
fn file_watch_dedup_identical_entries() {
    let reg = FileWatchRegistration {
        workflow_id: "wf-dup".to_string(),
        workflow_name: "Dup".to_string(),
        path: PathBuf::from("/tmp/dup"),
        recursive: true,
    };
    let mut regs = vec![reg.clone(), reg.clone()];
    regs.dedup_by(|l, r| l.workflow_id == r.workflow_id && l.path == r.path && l.recursive == r.recursive);
    assert_eq!(regs.len(), 1);
}

// ── Normalize event — comprehensive edge cases ─────────────────────────────────

#[test]
fn normalize_event_case_variants_create() {
    for variant in ["create", "CREATE", "Create", "cReAtE"] {
        assert_eq!(normalize_watch_event(variant), "create", "Failed for: {}", variant);
    }
}

#[test]
fn normalize_event_case_variants_modify() {
    for variant in ["modify", "MODIFY", "Modify"] {
        assert_eq!(normalize_watch_event(variant), "modify", "Failed for: {}", variant);
    }
}

#[test]
fn normalize_event_case_variants_remove() {
    for variant in ["remove", "REMOVE", "Remove"] {
        assert_eq!(normalize_watch_event(variant), "remove", "Failed for: {}", variant);
    }
}

#[test]
fn normalize_event_unknown_variants_are_any() {
    for variant in ["", "any", "delete", "unlink", "rename", "move", "attr"] {
        let result = normalize_watch_event(variant);
        assert_eq!(result, "any", "Expected 'any' for: {}", variant);
    }
}
