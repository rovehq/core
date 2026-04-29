//! Tests for memory::conductor::routing — ApexRoutingPolicy, route_for_step()

use rove_engine::memory::conductor::routing::ApexRoutingPolicy;
use rove_engine::memory::conductor::types::{PlanStep, RoutePolicy, StepRole, StepType};
use sdk::{Complexity, Route, TaskDomain};

fn make_step(role: StepRole) -> PlanStep {
    PlanStep {
        id: format!("step-{:?}", role),
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
        description: "work".to_string(),
        expected_outcome: "done".to_string(),
    }
}

// ── Researcher with local brain ────────────────────────────────────────────────

#[test]
fn researcher_local_brain_available_returns_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn researcher_local_brain_general_domain_returns_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::General,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn researcher_local_brain_data_domain_returns_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::Data,
        Complexity::Medium,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

// ── Researcher without local brain ─────────────────────────────────────────────

#[test]
fn researcher_no_local_brain_simple_returns_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::General,
        Complexity::Simple,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn researcher_no_local_brain_complex_cloud_domain_returns_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::Browser,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn researcher_no_local_brain_complex_data_cloud_returns_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::Data,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn researcher_no_local_brain_complex_general_prefers_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::General,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

// ── Verifier with local brain ──────────────────────────────────────────────────

#[test]
fn verifier_local_brain_returns_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Verifier),
        TaskDomain::Code,
        Complexity::Medium,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn verifier_local_brain_general_returns_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Verifier),
        TaskDomain::General,
        Complexity::Simple,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

// ── Verifier without local brain ────────────────────────────────────────────────

#[test]
fn verifier_no_local_brain_returns_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Verifier),
        TaskDomain::General,
        Complexity::Medium,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn verifier_no_local_brain_complex_cloud_browser_returns_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Verifier),
        TaskDomain::Browser,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

// ── Executor with local brain ──────────────────────────────────────────────────

#[test]
fn executor_local_preferred_uses_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Complex,
        Route::Local,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn executor_complex_ollama_preferred_local_brain_upgrades_to_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::General,
        Complexity::Complex,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn executor_simple_ollama_preferred_local_brain_keeps_ollama() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::General,
        Complexity::Simple,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

// ── Executor without local brain ────────────────────────────────────────────────

#[test]
fn executor_no_local_brain_local_preferred_falls_to_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Simple,
        Route::Local,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn executor_no_local_brain_ollama_preferred_keeps_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::General,
        Complexity::Medium,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn executor_cloud_complex_code_returns_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn executor_cloud_complex_browser_returns_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Browser,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn executor_cloud_complex_data_returns_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Data,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn executor_cloud_simple_general_returns_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::General,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn executor_cloud_medium_general_returns_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::General,
        Complexity::Medium,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);
}

// ── RoutePolicy overrides ──────────────────────────────────────────────────────

#[test]
fn local_only_policy_always_returns_local() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::LocalOnly,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn cloud_only_policy_always_returns_cloud() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::General,
        Complexity::Simple,
        Route::Local,
        &RoutePolicy::CloudOnly,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn local_preferred_policy_local_brain_returns_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::LocalPreferred,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn local_preferred_policy_no_local_brain_returns_ollama() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::LocalPreferred,
    );
    assert_eq!(route, Route::Ollama);
}

#[test]
fn cloud_only_policy_ignores_complexity() {
    let policy = ApexRoutingPolicy::new(true);
    for complexity in [Complexity::Simple, Complexity::Medium, Complexity::Complex] {
        let route = policy.route_for_step(
            &make_step(StepRole::Executor),
            TaskDomain::General,
            complexity,
            Route::Local,
            &RoutePolicy::CloudOnly,
        );
        assert_eq!(route, Route::Cloud);
    }
}

#[test]
fn local_only_policy_ignores_domain() {
    let policy = ApexRoutingPolicy::new(false);
    for domain in [
        TaskDomain::Code,
        TaskDomain::Browser,
        TaskDomain::Data,
        TaskDomain::General,
    ] {
        let route = policy.route_for_step(
            &make_step(StepRole::Executor),
            domain,
            Complexity::Complex,
            Route::Cloud,
            &RoutePolicy::LocalOnly,
        );
        assert_eq!(route, Route::Local);
    }
}

// ── Additional route_for_step tests via public API ────────────────────────────

#[test]
fn executor_no_local_brain_cloud_stays_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn verifier_local_brain_prefers_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Verifier),
        TaskDomain::Code,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn local_only_policy_overrides_cloud() {
    let policy = ApexRoutingPolicy::new(false);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::LocalOnly,
    );
    assert_eq!(route, Route::Local);
}

#[test]
fn cloud_only_policy_uses_cloud() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::Code,
        Complexity::Complex,
        Route::Cloud,
        &RoutePolicy::CloudOnly,
    );
    assert_eq!(route, Route::Cloud);
}

#[test]
fn local_preferred_with_local_brain_picks_local() {
    let policy = ApexRoutingPolicy::new(true);
    let route = policy.route_for_step(
        &make_step(StepRole::Researcher),
        TaskDomain::Code,
        Complexity::Simple,
        Route::Cloud,
        &RoutePolicy::LocalPreferred,
    );
    assert_eq!(route, Route::Local);
}

// ── RoutePolicy equality ────────────────────────────────────────────────────────

#[test]
fn route_policy_local_only_eq() {
    assert_eq!(RoutePolicy::LocalOnly, RoutePolicy::LocalOnly);
}

#[test]
fn route_policy_cloud_only_eq() {
    assert_eq!(RoutePolicy::CloudOnly, RoutePolicy::CloudOnly);
}

#[test]
fn route_policy_inherit_ne_local_only() {
    assert_ne!(RoutePolicy::Inherit, RoutePolicy::LocalOnly);
}

// ── Complexity variations for executor ────────────────────────────────────────

#[test]
fn executor_all_complexities_with_local_brain() {
    let policy = ApexRoutingPolicy::new(true);
    // Simple + Ollama preferred → Ollama (no upgrade)
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::General,
        Complexity::Simple,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);

    // Medium + Ollama → Ollama
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::General,
        Complexity::Medium,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Ollama);

    // Complex + Ollama → Local (upgrade)
    let route = policy.route_for_step(
        &make_step(StepRole::Executor),
        TaskDomain::General,
        Complexity::Complex,
        Route::Ollama,
        &RoutePolicy::Inherit,
    );
    assert_eq!(route, Route::Local);
}
