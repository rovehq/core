//! Integration test for dispatch brain → memory query flow
//!
//! Verifies that dispatch brain classification works correctly.

use brain::dispatch::DispatchBrain;
use sdk::{Complexity, Route, TaskDomain, ToolTag};

#[test]
fn test_dispatch_brain_initializes() {
    let brain = DispatchBrain::init();
    assert!(brain.is_ok());
}

#[test]
fn test_dispatch_classifies_git_task() {
    let brain = DispatchBrain::init().unwrap();
    let result = brain.classify("commit the changes to git");

    assert_eq!(result.domain, TaskDomain::Git);
    assert!(result.tools_needed.contains(&ToolTag::Git));
}

#[test]
fn test_dispatch_classifies_code_task() {
    let brain = DispatchBrain::init().unwrap();
    let result = brain.classify("write a rust function to parse JSON");

    assert_eq!(result.domain, TaskDomain::Code);
    assert!(result.tools_needed.contains(&ToolTag::Filesystem));
}

#[test]
fn test_dispatch_classifies_shell_task() {
    let brain = DispatchBrain::init().unwrap();
    let result = brain.classify("list files in the current directory");

    assert_eq!(result.domain, TaskDomain::Shell);
    assert!(result.tools_needed.contains(&ToolTag::Terminal));
}

#[test]
fn test_dispatch_detects_sensitive() {
    let brain = DispatchBrain::init().unwrap();
    let result = brain.classify("show me the API key from the config");

    assert!(result.sensitive);
    assert_eq!(result.route, Route::Local);
}

#[test]
fn test_dispatch_detects_complexity() {
    let brain = DispatchBrain::init().unwrap();

    let simple = brain.classify("what is the current time");
    assert_eq!(simple.complexity, Complexity::Simple);

    let medium = brain.classify("build the project and then run tests");
    assert_eq!(medium.complexity, Complexity::Medium);

    let complex = brain.classify("plan a multi-step deployment");
    assert_eq!(complex.complexity, Complexity::Complex);
}

#[test]
fn test_dispatch_default_route() {
    let brain = DispatchBrain::init().unwrap();
    let result = brain.classify("hello world");

    // Non-sensitive tasks default to Ollama
    assert!(!result.sensitive);
    assert_eq!(result.route, Route::Ollama);
}
