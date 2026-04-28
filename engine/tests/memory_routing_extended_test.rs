//! Extended tests for memory::conductor::routing — additional route_for_step combinations

use rove_engine::conductor::types::{PlanStep, RoutePolicy, StepRole, StepType};
use rove_engine::conductor::routing::ApexRoutingPolicy;
use sdk::{Complexity, Route, TaskDomain};

fn make_step_with_role(role: StepRole) -> PlanStep {
    PlanStep {
        id: "step-1".to_string(),
        order: 0,
        step_type: match role {
            StepRole::Researcher => StepType::Research,
            StepRole::Executor => StepType::Execute,
            StepRole::Verifier => StepType::Verify,
        },
        role,
        parallel_safe: false,
        route_policy: RoutePolicy::Inherit,
        dependencies: Vec::new(),
        description: "do work".to_string(),
        expected_outcome: "done".to_string(),
    }
}

// ── LocalOnly policy ──────────────────────────────────────────────────────────

#[test]
fn local_only_policy_returns_local_regardless_of_brain() {
    for brain_available in [true, false] {
        let policy = ApexRoutingPolicy::new(brain_available);
        let step = make_step_with_role(StepRole::Executor);
        let route = policy.route_for_step(
            &step,
            TaskDomain::Code,
            Complexity::Complex,
            Route::Cloud,
            &RoutePolicy::LocalOnly,
        );
        assert_eq!(route, Route::Local, "LocalOnly failed for brain_available={}", brain_available);
    }
}

#[test]
fn local_only_researcher_returns_local() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::LocalOnly,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn local_only_verifier_returns_local() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Verifier);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Medium,
        Route::Ollama,
        &RoutePolicy::LocalOnly,
    );
    assert_eq!(route, Route::Local);
}

// ── CloudOnly policy ──────────────────────────────────────────────────────────

#[test]
fn cloud_only_policy_returns_cloud_regardless_of_brain() {
    for brain_available in [true, false] {
        let policy = ApexRoutingPolicy::new(brain_available);
        let step = make_step_with_role(StepRole::Executor);
        let route = policy.route_for_step(
            &step,
            TaskDomain::Code,
            Complexity::Simple,
            Route::Ollama,
            &RoutePolicy::CloudOnly,
        );
        assert_eq!(route, Route::Cloud, "CloudOnly failed for brain_available={}", brain_available);
    }
}

#[test]
fn cloud_only_researcher_returns_cloud() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Browser,
        Complexity::Complex,
        Route::Local,
        &RoutePolicy::CloudOnly,
    );
    assert_eq!(route, Route::Cloud);
}

// ── LocalPreferred policy ─────────────────────────────────────────────────────

#[test]
fn local_preferred_with_brain_returns_local() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::LocalPreferred,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn local_preferred_without_brain_returns_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::LocalPreferred,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn local_preferred_executor_with_brain_returns_local() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::LocalPreferred,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn local_preferred_executor_without_brain_returns_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::LocalPreferred,
    );
    assert_eq!(route, Route::Ollama);
}

// ── Inherit: Researcher ───────────────────────────────────────────────────────

#[test]
fn inherit_researcher_local_brain_available_gives_local() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn inherit_researcher_no_brain_complex_cloud_domain_gives_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Browser,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn inherit_researcher_no_brain_simple_general_gives_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn inherit_researcher_no_brain_complex_data_domain_gives_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Data,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn inherit_researcher_no_brain_complex_code_ollama_preferred_gives_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Researcher);
    // preferred_route=Ollama, not Cloud → falls through to Ollama
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Complex,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

// ── Inherit: Verifier ─────────────────────────────────────────────────────────

#[test]
fn inherit_verifier_with_brain_gives_local() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Verifier);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn inherit_verifier_no_brain_simple_gives_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Verifier);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

// ── Inherit: Executor ─────────────────────────────────────────────────────────

#[test]
fn inherit_executor_preferred_local_with_brain_gives_local() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Medium,
        Route::Local,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn inherit_executor_preferred_local_without_brain_gives_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Complex,
        Route::Local,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn inherit_executor_preferred_ollama_complex_with_brain_upgrades_to_local() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Complex,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn inherit_executor_preferred_ollama_simple_without_brain_stays_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Simple,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn inherit_executor_preferred_cloud_complex_code_gives_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn inherit_executor_preferred_cloud_complex_browser_gives_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Browser,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn inherit_executor_preferred_cloud_complex_data_gives_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Data,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn inherit_executor_preferred_cloud_simple_general_gives_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn inherit_executor_preferred_cloud_medium_general_gives_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Medium,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

// ── Additional route_for_step via public API ──────────────────────────────────

#[test]
fn researcher_with_local_brain_ollama_preferred_picks_local() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Researcher);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Simple,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn executor_no_local_brain_cloud_stays_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn verifier_local_brain_simple_picks_local() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Verifier);
    let route = policy.route_for_step(
        &step,
        TaskDomain::Code,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn local_only_policy_all_roles_return_local() {
    let policy = ApexRoutingPolicy::new(false);
    for role in [StepRole::Researcher, StepRole::Executor, StepRole::Verifier] {
        let step = make_step_with_role(role.clone());
        let route = policy.route_for_step(
            &step,
            TaskDomain::Code,
            Complexity::Complex,
            Route::Cloud,
            &RoutePolicy::LocalOnly,
        );
        assert_eq!(route, Route::Local, "Expected Local for {:?}", role);
    }
}

#[test]
fn cloud_only_policy_all_roles_return_cloud() {
    let policy = ApexRoutingPolicy::new(true);
    for role in [StepRole::Researcher, StepRole::Executor, StepRole::Verifier] {
        let step = make_step_with_role(role.clone());
        let route = policy.route_for_step(
            &step,
            TaskDomain::Code,
            Complexity::Complex,
            Route::Cloud,
            &RoutePolicy::CloudOnly,
        );
        assert_eq!(route, Route::Cloud, "Expected Cloud for {:?}", role);
    }
}

#[test]
fn executor_complex_ollama_upgrades_to_local_with_brain() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Complex,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn executor_simple_ollama_stays_ollama_with_brain() {
    let policy = ApexRoutingPolicy::new(true);
    let step = make_step_with_role(StepRole::Executor);
    let route = policy.route_for_step(
        &step,
        TaskDomain::General,
        Complexity::Simple,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

// ── ApexRoutingPolicy construction ────────────────────────────────────────────

#[test]
fn routing_policy_constructs_with_local_brain() {
    let _ = ApexRoutingPolicy::new(true);
}

#[test]
fn routing_policy_constructs_without_local_brain() {
    let _ = ApexRoutingPolicy::new(false);
}
