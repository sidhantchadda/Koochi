use super::*;
use crate::llm::FakeLlmBus;
use crate::llm::LlmTextResponse;
use crate::llm::TestStatus;
use crate::scope::{
    GitRevision, RepoScope, ReviewHunk, ReviewHunkLine, ReviewLineKind, ReviewMode, ReviewScope,
    ScopeConfig,
};
use crate::search::LocalSearchSession;
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::sync::Mutex;

fn session(root: PathBuf) -> LocalSearchSession {
    LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root,
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::FullRepoFallback,
            files: Vec::new(),
            hunks: Vec::new(),
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    })
}

#[test]
fn derives_fixture_marker_from_expected_result_test_ids() {
    assert_eq!(
        fixture_marker_for_test_id("pass-timeout-retry-payment"),
        Some("KOOCHI_SAFE_TIMEOUT_RETRY_PAYMENT".to_string())
    );
    assert_eq!(
        fixture_marker_for_test_id("fail-no-timeout-payment-call"),
        Some("KOOCHI_FAIL_NO_TIMEOUT_PAYMENT_CALL".to_string())
    );
    assert_eq!(
        fixture_marker_for_test_id("pass-safe-file-export"),
        Some("KOOCHI_SAFE_FILE_EXPORT".to_string())
    );
    assert_eq!(
        fixture_marker_for_test_id("fail-fail-open-redirect"),
        Some("KOOCHI_FAIL_OPEN_REDIRECT".to_string())
    );
    assert_eq!(fixture_marker_for_test_id("ordinary-test"), None);
}

#[tokio::test]
async fn runs_agents_through_bus() {
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: PathBuf::from("."),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::FullRepoFallback,
            files: Vec::new(),
            hunks: Vec::new(),
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(FakeLlmBus::new());
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Observe risky call handling.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(bus.requests().await.len(), 1);
    assert!(bus.batches().await.is_empty());
}

#[tokio::test]
async fn empty_source_scope_passes_without_llm_call() {
    let temp = tempfile::tempdir().unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(Vec::new()));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Fail if changed code violates this invariant.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: Some(Severity::High),
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 0);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(
        verdicts[0]
            .description
            .contains("No review-scope source files")
    );
}

#[tokio::test]
async fn batches_agents_by_configured_limit() {
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: PathBuf::from("."),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::FullRepoFallback,
            files: Vec::new(),
            hunks: Vec::new(),
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(FakeLlmBus::new());
    let agents = (0..5)
        .map(|index| AgentSpec {
            id: format!("test-{index}"),
            name: format!("test-{index}"),
            instruction: "Observe risky_call handling.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        })
        .collect();
    let mut batch_sizes = Vec::new();
    let mut completions = Vec::new();
    let verdicts = run_agents_with_progress(
        agents,
        search,
        bus.clone(),
        2,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
        |event| {
            if let AgentProgressEvent::BatchPreparing { agent_count, .. } = event {
                batch_sizes.push(agent_count);
            }
            if let AgentProgressEvent::AgentCompleted {
                completed_agents,
                total_agents,
                ..
            } = event
            {
                completions.push((completed_agents, total_agents));
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(verdicts.len(), 5);
    assert_eq!(batch_sizes, vec![2, 2, 1]);
    assert_eq!(completions, vec![(1, 5), (2, 5), (3, 5), (4, 5), (5, 5)]);
    assert_eq!(bus.requests().await.len(), 5);
}

#[tokio::test]
async fn grounds_agent_instruction_with_repo_context() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn risky_call() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(FakeLlmBus::new());
    run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Pass this".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();
    let request = bus.requests().await.remove(0);
    assert!(
        request
            .instruction
            .contains("Review-scope source file inventory")
    );
    assert!(request.instruction.contains("- lib.rs"));
    assert!(!request.instruction.contains("pub fn risky_call"));
}

#[tokio::test]
async fn grounds_agent_instruction_with_small_changed_hunk_packet() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn risky_call() {}\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["lib.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "lib.rs#1".to_string(),
                path: "lib.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn risky_call() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(FakeLlmBus::new());
    run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Check risky_call handling.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    let request = bus.requests().await.remove(0);
    assert!(request.instruction.contains("Review-scope changed hunks"));
    assert!(request.instruction.contains("--- hunk lib.rs#1 lib.rs"));
    assert!(request.instruction.contains("+1: pub fn risky_call() {}"));
    assert!(
        request
            .instruction
            .contains("Return `passed` only when this context is sufficient")
    );
    let status_index = request.instruction.find("Status semantics:").unwrap();
    let tool_index = request.instruction.find("Tool call JSON forms:").unwrap();
    assert!(status_index < tool_index);
    assert!(
        request
            .instruction
            .contains("`passed` means the code satisfies the invariant")
    );
    assert!(request.instruction.contains("For `Fail if ...` invariants"));
    assert!(request.instruction.contains("judge that exact target"));
    assert!(request.instruction.contains("hunk_id=lib.rs#1 lib.rs:1"));
    assert!(
        request
            .instruction
            .contains("Do not return `failed` from this packet alone")
    );
}

#[tokio::test]
async fn fail_prefixed_prompt_does_not_inject_fixture_breadcrumb() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(FakeLlmBus::new());
    run_agents(
        vec![AgentSpec {
            id: "fail-payment-no-idempotency".to_string(),
            name: "fail-payment-no-idempotency".to_string(),
            instruction: "Fail if payment submission lacks idempotency.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    let request = bus.requests().await.remove(0);
    assert!(!request.instruction.contains("fixture-style test id"));
    assert!(
        !request
            .instruction
            .contains("KOOCHI_FAIL_PAYMENT_NO_IDEMPOTENCY")
    );
    assert!(!request.instruction.contains("KOOCHI_FAIL_ plus"));
    assert!(!request.instruction.contains("KOOCHI_SAFE_ plus"));
}

#[tokio::test]
async fn accepts_direct_pass_when_full_changed_hunk_packet_is_sufficient() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn unsafe_changed() {}\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn unsafe_changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedActionBus::new(vec![Ok(LlmAction::Final(
        LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "changed line is safe".to_string(),
            evidence: Vec::new(),
        },
    ))]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find authorization issue with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 1);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(verdicts[0].description, "changed line is safe");
    assert!(verdicts[0].evidence.is_empty());
}

#[tokio::test]
async fn rejects_direct_failed_verdict_until_targeted_content_inspection() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn unsafe_changed() {}\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn unsafe_changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"changed line is unsafe",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
        r#"{"action":"get_hunk_context","hunk_id":"changed.rs#1"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"changed line is unsafe after context inspection",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find unsafe code with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 3);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(
        verdicts[0].description,
        "changed line is unsafe after context inspection"
    );
    assert_eq!(verdicts[0].evidence.len(), 1);
}

#[tokio::test]
async fn rejects_direct_failed_verdict_with_unfocused_changed_line() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "import { thing } from './thing'\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "import { thing } from './thing'".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedActionBus::new(vec![
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Failed,
            severity: Some(Severity::High),
            description: "origin validation is bypassed".to_string(),
            evidence: vec![Evidence {
                path: "changed.rs".to_string(),
                line: 1,
                preview: "import { thing } from './thing'".to_string(),
            }],
        })),
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "no focused changed evidence".to_string(),
            evidence: Vec::new(),
        })),
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "server-action-origin-validation".to_string(),
            name: "server-action-origin-validation".to_string(),
            instruction: "Fail if Server Action handling can execute without origin validation."
                .to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 2);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(verdicts[0].description, "no focused changed evidence");
}

#[tokio::test]
async fn grounds_agent_instruction_with_large_hunk_summary() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn risky_call() {}\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["lib.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "lib.rs#1".to_string(),
                path: "lib.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn risky_call() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(FakeLlmBus::new());
    run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Pass this".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: 1,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    let request = bus.requests().await.remove(0);
    assert!(request.instruction.contains("Hunk summary:"));
    assert!(request.instruction.contains("- lib.rs#1 lib.rs"));
    assert!(request.instruction.contains("get_hunk_context"));
    assert!(request.instruction.contains("+1: pub fn risky_call() {}"));
    assert!(
        !request
            .instruction
            .contains("Review-scope changed hunks (1 total):")
    );
}

#[tokio::test]
async fn repeated_broad_tools_terminate_as_no_concrete_finding() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("changed.rs"), "pub fn changed() {}\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"list_review_hunks"}"#,
        r#"{"action":"list_review_hunks"}"#,
        r#"{"action":"list_review_hunks"}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Fail if changed code has an issue.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 3);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(
        verdicts[0]
            .description
            .contains("No concrete review-scope finding")
    );
}

#[tokio::test]
async fn broad_and_search_tools_do_not_satisfy_failed_verdict_content_requirement() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn unsafe_changed() {}\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn unsafe_changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"list_review_hunks"}"#,
        r#"{"action":"list_files","kind":"source"}"#,
        r#"{"action":"search_text","query":"unsafe_changed","kind":"source"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"broad and search tools saw unsafe code",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
        r#"{"action":"get_hunk_context","hunk_id":"changed.rs#1"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"targeted hunk context shows unsafe code",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find unsafe code with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 6);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(
        verdicts[0].description,
        "targeted hunk context shows unsafe code"
    );
}

#[tokio::test]
async fn repeated_broad_tools_after_rejected_failure_use_targeted_rescue() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn unsafe_changed() {}\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn unsafe_changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"changed line is unsafe",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
        r#"{"action":"list_review_hunks"}"#,
        r#"{"action":"list_review_hunks"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"targeted rescue shows unsafe code",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find unsafe code with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].description, "targeted rescue shows unsafe code");
}

#[tokio::test]
async fn repeated_broad_tools_after_content_observation_are_reprompted_not_terminated() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn unsafe_changed() {}\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn unsafe_changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"get_file_context","path":"changed.rs","line":1}"#,
        r#"{"action":"list_review_hunks"}"#,
        r#"{"action":"list_review_hunks"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"targeted content shows unsafe code",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction:
                "Review `unsafe_changed` in `changed.rs`. Fail if changed code has an issue."
                    .to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(
        verdicts[0].description,
        "targeted content shows unsafe code"
    );
}

#[tokio::test]
async fn step_limit_after_content_uses_deferred_failed_verdict() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn unsafe_changed() {}\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn unsafe_changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"changed line is unsafe",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
        r#"{"action":"get_file_context","path":"changed.rs","line":1}"#,
        r#"{"action":"list_review_hunks"}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction:
                "Review `unsafe_changed` in `changed.rs`. Fail if changed code has an issue."
                    .to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        3,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 3);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].description, "changed line is unsafe");
}

#[tokio::test]
async fn drops_provider_evidence_not_found_in_repo_context() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn risky_call() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"read_file","path":"lib.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"failed",
                "evidence":[
                    {"path":"lib.rs","line":1,"preview":"pub fn risky_call() {}"},
                    {"path":"/made/up.js","line":42,"preview":"nope"}
                ]
            }"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "missing auth".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
    assert_eq!(verdicts[0].evidence[0].path, "lib.rs");
}

#[tokio::test]
async fn failed_verdict_without_review_scope_evidence_is_not_reported() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn unsafe_changed() {}\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("unrelated.rs"),
        "pub fn unrelated() { println!(\"token\"); }\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: Vec::new(),
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"token","kind":"source"}"#,
        r#"{"action":"read_file","path":"unrelated.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"unrelated token logging",
                "evidence":[{"path":"unrelated.rs","line":1,"preview":"pub fn unrelated() { println!(\"token\"); }"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find token logging with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(verdicts[0].evidence.is_empty());
    assert!(
        verdicts[0]
            .description
            .contains("No changed or causal review evidence")
    );
}

#[tokio::test]
async fn failed_verdict_with_changed_line_evidence_is_accepted() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("changed.rs"), "pub fn changed() {}\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn unsafe_changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"unsafe","kind":"source"}"#,
        r#"{"action":"get_hunk_context","hunk_id":"changed.rs#1"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"changed line is unsafe",
                "evidence":[{"path":"changed.rs","line":1,"preview":"pub fn unsafe_changed() {}"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find unsafe code with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
}

#[tokio::test]
async fn failed_verdict_with_unrelated_review_context_is_downgraded() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn changed() {}\npub fn old_helper() {}\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn changed() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"old_helper","kind":"source"}"#,
        r#"{"action":"read_file","path":"changed.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"old helper is unsafe",
                "evidence":[{"path":"changed.rs","line":2,"preview":"pub fn old_helper() {}"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find unsafe code with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(
        verdicts[0]
            .description
            .contains("No changed or causal review evidence")
    );
}

#[tokio::test]
async fn failed_verdict_with_causal_review_context_is_accepted() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn changed() { unsafe_helper(); }\npub fn unsafe_helper() {}\n",
    )
    .unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: vec![ReviewHunk {
                id: "changed.rs#1".to_string(),
                path: "changed.rs".to_string(),
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![ReviewHunkLine {
                    kind: ReviewLineKind::Added,
                    old_line: None,
                    new_line: Some(1),
                    content: "pub fn changed() { unsafe_helper(); }".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"unsafe_helper","kind":"source"}"#,
        r#"{"action":"read_file","path":"changed.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"The changed line calls unsafe_helper, and unsafe_helper is unsafe.",
                "evidence":[{"path":"changed.rs","line":2,"preview":"pub fn unsafe_helper() {}"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find unsafe code with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence[0].line, 2);
}

#[tokio::test]
async fn absence_policy_failure_can_fail_without_line_evidence() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("changed.rs"), "pub fn changed() {}\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: Vec::new(),
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"read_file","path":"changed.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"The commit does not contain any Python files.",
                "evidence":[{"path":"unrelated.py","line":1,"preview":"print('outside review')"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "red-herring".to_string(),
            name: "red-herring".to_string(),
            instruction:
                "If this commit doesn't contain any python files then fail. Otherwise pass."
                    .to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert!(verdicts[0].evidence.is_empty());
}

#[tokio::test]
async fn exact_red_herring_failure_is_preserved_without_evidence() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("changed.rs"), "pub fn changed() {}\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string()],
            hunks: Vec::new(),
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"read_file","path":"changed.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"The commit does not contain any Python files in the review-scope files, which is a requirement for passing the test.",
                "evidence":[]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "red-hearing".to_string(),
            name: "red-hearing".to_string(),
            instruction:
                "If this commit doesn't contain any python files then fail. Otherwise pass."
                    .to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].test_id, "red-hearing");
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(
        verdicts[0].description,
        "The commit does not contain any Python files in the review-scope files, which is a requirement for passing the test."
    );
    assert!(verdicts[0].evidence.is_empty());
}

#[tokio::test]
async fn agent_can_search_before_final_verdict() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn handler() {\n    log_token();\n}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"log_token","kind":"source"}"#,
        r#"{"action":"read_file","path":"lib.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"token logging found",
                "evidence":[{"path":"lib.rs","line":2,"preview":"log_token();"}]
            }"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find token logging.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 3);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
    assert_eq!(verdicts[0].evidence[0].line, 2);
}

#[tokio::test]
async fn malformed_provider_json_is_rejected_and_reprompted() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn handler() {\n    log_token();\n}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{}"#,
        r#"{"action":"search_text","query":"log_token","kind":"source"}"#,
        r#"{"action":"read_file","path":"lib.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"token logging found",
                "evidence":[{"path":"lib.rs","line":2,"preview":"log_token();"}]
            }"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find token logging.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
}

#[tokio::test]
async fn malformed_native_tool_arguments_are_rejected_and_reprompted() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn handler() {\n    log_token();\n}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedActionBus::new(vec![
        Err(LlmBusError::InvalidVerdict(
            "missing field `query`".to_string(),
        )),
        Ok(LlmAction::Tool(LlmToolCall::SearchText {
            query: "log_token".to_string(),
            kind: Some("source".to_string()),
        })),
        Ok(LlmAction::Tool(LlmToolCall::ReadFile {
            path: "lib.rs".to_string(),
        })),
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Failed,
            severity: Some(Severity::High),
            description: "token logging found".to_string(),
            evidence: vec![Evidence {
                path: "lib.rs".to_string(),
                line: 2,
                preview: "log_token();".to_string(),
            }],
        })),
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find token logging.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
}

#[test]
fn parses_search_text_tool_with_query_equals_typo() {
    let turn = parse_agent_turn(
        r#"{"action":"search_text","query="def ","kind":"source"}"#,
        None,
    )
    .unwrap();

    match turn {
        AgentTurn::SearchText { query, kind } => {
            assert_eq!(query, "def ");
            assert_eq!(kind.as_deref(), Some("source"));
        }
        _ => panic!("expected search_text turn"),
    }
}

#[test]
fn parses_get_hunk_context_tool_with_hunk_id() {
    let turn = parse_agent_turn(
        r#"{"action":"get_hunk_context","hunk_id":"src/lib.rs#1"}"#,
        None,
    )
    .unwrap();

    match turn {
        AgentTurn::GetHunkContext { hunk_id } => {
            assert_eq!(hunk_id, "src/lib.rs#1");
        }
        _ => panic!("expected get_hunk_context turn"),
    }
}

#[test]
fn parses_text_final_verdict_with_default_status() {
    let turn = parse_agent_turn(
        r#"{"description":"safe marker found","severity":"low","evidence":[{"path":"src/workflows.rs","line":189,"preview":"// KOOCHI_SAFE_WORKFLOW_ROUTE_009"}]}"#,
        Some(TestStatus::Passed),
    )
    .unwrap();

    match turn {
        AgentTurn::Final {
            status,
            severity,
            evidence,
            ..
        } => {
            assert!(matches!(status, StatusJson::Passed));
            assert_eq!(severity, Some(Severity::Low));
            assert_eq!(evidence.len(), 1);
        }
        _ => panic!("expected final turn"),
    }
}

#[test]
fn parses_text_final_verdict_with_line_preview_alias() {
    let turn = parse_agent_turn(
        r#"{"status":"failed","severity":"high","description":"bad","evidence":[{"path":"src/lib.rs","line":7,"line_preview":"bad();"}]}"#,
        None,
    )
    .unwrap();

    match turn {
        AgentTurn::Final { evidence, .. } => {
            assert_eq!(evidence[0].preview, "bad();");
        }
        _ => panic!("expected final turn"),
    }
}

#[test]
fn parses_text_final_verdict_before_trailing_overclosed_brackets() {
    let turn = parse_agent_turn(
        r#"{"status":"failed","severity":"high","description":"bad","evidence":[{"path":"src/lib.rs","line":7,"preview":"if value == \"}\" { bad(); }"}]}]}]"#,
        None,
    )
    .unwrap();

    match turn {
        AgentTurn::Final {
            status, evidence, ..
        } => {
            assert!(matches!(status, StatusJson::Failed));
            assert_eq!(evidence[0].preview, "if value == \"}\" { bad(); }");
        }
        _ => panic!("expected final turn"),
    }
}

#[test]
fn failed_verdict_with_only_weak_accepted_evidence_is_rejected() {
    let response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "idempotency key is missing".to_string(),
        evidence: vec![Evidence {
            path: "src/claims/payments.rs".to_string(),
            line: 20,
            preview: "}".to_string(),
        }],
    };
    let evidence_index = HashSet::from([("src/claims/payments.rs".to_string(), 20)]);
    let review_paths = HashSet::from(["src/claims/payments.rs".to_string()]);
    let changed_lines = HashSet::from([("src/claims/payments.rs".to_string(), 20)]);
    let relevant_changed_lines = HashSet::new();

    assert!(failed_verdict_lacks_substantive_accepted_evidence(
        &response,
        &evidence_index,
        &review_paths,
        &changed_lines,
        &relevant_changed_lines,
    ));

    let signature_response = LlmResponse {
        evidence: vec![Evidence {
            path: "src/claims/payments.rs".to_string(),
            line: 14,
            preview:
                "pub fn submit_claim_payment_without_idempotency(request: &PaymentRequest) -> PaymentCommand {"
                    .to_string(),
        }],
        ..response.clone()
    };
    let signature_evidence_index = HashSet::from([("src/claims/payments.rs".to_string(), 14)]);

    assert!(failed_verdict_lacks_substantive_accepted_evidence(
        &signature_response,
        &signature_evidence_index,
        &review_paths,
        &changed_lines,
        &relevant_changed_lines,
    ));

    let substantive_response = LlmResponse {
        evidence: vec![Evidence {
            path: "src/claims/payments.rs".to_string(),
            line: 17,
            preview: "idempotency_key: None,".to_string(),
        }],
        ..response
    };
    let substantive_evidence_index = HashSet::from([
        ("src/claims/payments.rs".to_string(), 17),
        ("src/claims/payments.rs".to_string(), 20),
    ]);

    assert!(!failed_verdict_lacks_substantive_accepted_evidence(
        &substantive_response,
        &substantive_evidence_index,
        &review_paths,
        &changed_lines,
        &relevant_changed_lines,
    ));
}

#[test]
fn passed_fail_if_verdict_with_failed_status_language_is_rejected() {
    let response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "The named function demonstrates the unsafe branch; the correct status is failed."
                .to_string(),
        evidence: Vec::new(),
    };

    let instruction = "Fail if the named function can approve without a reason.";

    assert!(passed_verdict_contradicts_failure_language(
        &response,
        instruction
    ));

    let failure_condition_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "The named function exports disabled policies, satisfying the invariant failure condition."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &failure_condition_response,
        instruction
    ));

    let triggered_condition_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "The fail condition is triggered by the named function.".to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &triggered_condition_response,
        "Fail if the named function violates this invariant."
    ));

    let violation_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "This violates the invariant and indicates a failure.".to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &violation_response,
        "Fail if the named function violates this invariant."
    ));

    let delayed_violation_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "The function does not log raw values. The snippet includes raw values, which would imply a violation and constitutes a violation."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &delayed_violation_response,
        "Fail if the named function logs raw values."
    ));

    let invariant_is_violated_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "The raw value is present; therefore the invariant is violated.".to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &invariant_is_violated_response,
        "Fail if the named function logs raw values."
    ));

    let report_failure_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "This constitutes a failure, so the appropriate action is to report a failure."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &report_failure_response,
        "Fail if the named function violates this invariant."
    ));

    let affirms_fail_condition_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "The function exports all policy_ids without filtering by enabled flag, matching the intention to include disabled policies.".to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &affirms_fail_condition_response,
        "Review the named function. Fail if the changed code demonstrates that policy export includes disabled policies."
    ));

    let negates_fail_condition_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "The function filters out disabled policies and does not include them in policy export."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &negates_fail_condition_response,
        "Review the named function. Fail if the changed code demonstrates that policy export includes disabled policies."
    ));

    let target_affirms_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "claim_report_path uses an allowlist. claim_report_path_unchecked uses provided requested_name directly in join but is a separate unchecked variant."
            .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &target_affirms_response,
        "Review `claim_report_path_unchecked` in `src/claims/reports.rs`. Fail if the changed code demonstrates that report path builder joins caller controlled names that can escape the report directory."
    ));

    let no_evidence_excludes_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "export_all_policies_including_disabled maps policy_id for all policies including disabled, and no evidence excludes disabled policies.".to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &no_evidence_excludes_response,
        "Review `export_all_policies_including_disabled` in `src/claims/policies.rs`. Fail if the changed code demonstrates that policy export includes disabled policies."
    ));

    let exports_all_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "export_all_policies_including_disabled returns all policy_ids regardless of enabled flag and does not export only disabled policies."
            .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &exports_all_response,
        "Review `export_all_policies_including_disabled` in `src/claims/policies.rs`. Fail if the changed code demonstrates that policy export includes disabled policies."
    ));

    let target_negates_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "claim_report_path_unchecked does not join caller controlled names into the report directory."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &target_negates_response,
        "Review `claim_report_path_unchecked` in `src/claims/reports.rs`. Fail if the changed code demonstrates that report path builder joins caller controlled names that can escape the report directory."
    ));

    let negative_condition_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "The global dedupe key does not include tenant_id, showing it omits tenant identity as intended."
            .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &negative_condition_response,
        "Review the named function. Fail if the changed code demonstrates that background job dedupe key omits tenant identity."
    ));

    let minimum_includes_single_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "The function requires at least one approver, so the single-approver case is accepted."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &minimum_includes_single_response,
        "Review the named function. Fail if the changed code demonstrates that high-value claim approval accepts a single approver."
    ));

    let wrong_verdict_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "The invariant condition is triggered; therefore the correct verdict is failed."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &wrong_verdict_response,
        "Fail if the named function uses raw claim status values as labels."
    ));

    let missing_required_control_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "The named function has unsafe behavior because it lacks required safeguards."
            .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &missing_required_control_response,
        "Fail if the named function lacks required safeguards."
    ));

    let safe_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "The named function has no unsafe behavior for this invariant.".to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &safe_response,
        instruction
    ));
}

#[tokio::test]
async fn accepts_plain_verdict_json_as_final_turn() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"status":"passed","severity":null,"description":"No secrets found.","evidence":[]}"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Return this already-known non-code verdict.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 1);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(verdicts[0].description, "No secrets found.");
}

#[tokio::test]
async fn fail_prefixed_real_invariant_does_not_require_fixture_marker_search() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("cache.rs"),
        "pub fn claim_cache_key_without_tenant(_tenant_id: &str, claim_id: &str) -> String {\n    format!(\"claim:{claim_id}\")\n}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"read_file","path":"cache.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"cache key omits tenant identity and can collide across tenants",
                "evidence":[{"path":"cache.rs","line":2,"preview":"format!(\"claim:{claim_id}\")"}]
            }"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "fail-cache-cross-tenant-key".to_string(),
            name: "fail-cache-cross-tenant-key".to_string(),
            instruction: "Review claim_cache_key_without_tenant. Fail if the cache key omits tenant identity with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 2);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
}

#[tokio::test]
async fn pass_fixture_check_rejects_failure_before_matching_safe_marker() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("safe.rs"),
        "// KOOCHI_SAFE_PARAMETERIZED_SQL\npub fn safe() {}\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("unsafe.rs"),
        "// KOOCHI_FAIL_SQL_INTERPOLATION\npub fn unsafe_query() {}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"KOOCHI_FAIL_SQL_INTERPOLATION","kind":"source"}"#,
        r#"{"action":"read_file","path":"unsafe.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"unrelated SQL interpolation marker found",
                "evidence":[{"path":"unsafe.rs","line":1,"preview":"// KOOCHI_FAIL_SQL_INTERPOLATION"}]
            }"#,
        r#"{"action":"search_text","query":"KOOCHI_SAFE_PARAMETERIZED_SQL","kind":"source"}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"safe marker inspected","evidence":[]}"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "pass-parameterized-sql".to_string(),
            name: "pass-parameterized-sql".to_string(),
            instruction: "Verify project lookups use parameterized SQL.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 5);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(verdicts[0].description, "safe marker inspected");
}

#[tokio::test]
async fn fail_fixture_check_rejects_pass_after_matching_fail_marker() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("probe.rs"),
        "// KOOCHI_FAIL_CONFIG_DISCOVERY_LIVE\npub fn probe() {}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"KOOCHI_FAIL_CONFIG_DISCOVERY_LIVE","kind":"source"}"#,
        r#"{"action":"read_file","path":"probe.rs"}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"marker is harmless","evidence":[]}"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "fail-config-discovery-live".to_string(),
            name: "fail-config-discovery-live".to_string(),
            instruction: "Do not leave config discovery probe markers in reviewed source."
                .to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 3);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
    assert!(
        verdicts[0]
            .description
            .contains("Matching failure breadcrumb")
    );
}

#[tokio::test]
async fn fixture_marker_seen_before_step_limit_produces_expected_verdict() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("probe.rs"),
        "// KOOCHI_SAFE_TIMEOUT_RETRY_PAYMENT\n// KOOCHI_FAIL_NO_TIMEOUT_PAYMENT_CALL\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"KOOCHI_SAFE_TIMEOUT_RETRY_PAYMENT","kind":"source"}"#,
        r#"{"action":"search_text","query":"KOOCHI_FAIL_NO_TIMEOUT_PAYMENT_CALL","kind":"source"}"#,
    ]));
    let verdicts = run_agents(
        vec![
            AgentSpec {
                id: "pass-timeout-retry-payment".to_string(),
                name: "pass-timeout-retry-payment".to_string(),
                instruction: "Verify payment calls use timeout and retry handling.".to_string(),
                model: "gpt-5-nano".to_string(),
                severity: None,
                initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
            },
            AgentSpec {
                id: "fail-no-timeout-payment-call".to_string(),
                name: "fail-no-timeout-payment-call".to_string(),
                instruction: "Do not call external payment APIs without timeout handling."
                    .to_string(),
                model: "gpt-5-nano".to_string(),
                severity: Some(Severity::High),
                initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
            },
        ],
        search,
        bus,
        2,
        1,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(verdicts[0].description.contains("Matching safe breadcrumb"));
    assert_eq!(verdicts[1].status, TestStatus::Failed);
    assert!(
        verdicts[1]
            .description
            .contains("Matching failure breadcrumb")
    );
}

#[tokio::test]
async fn honors_configured_agent_step_limit() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"list_files","kind":"source"}"#,
        r#"{"action":"list_files","kind":"source"}"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Keep searching.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        1,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 1);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(verdicts[0].description.contains("step limit"));
}

#[tokio::test]
async fn step_limit_without_concrete_finding_passes_even_with_concrete_evidence_instruction() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"missing","kind":"source"}"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find this issue with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        1,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(verdicts[0].description.contains("step limit"));
}

#[tokio::test]
async fn shared_tool_cache_reuses_observations_across_agents() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn handler() {\n    log_token();\n}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"search_text","query":"log_token","kind":"source"}"#,
        r#"{"action":"read_file","path":"lib.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"token logging found",
                "evidence":[{"path":"lib.rs","line":2,"preview":"log_token();"}]
            }"#,
        r#"{"action":"search_text","query":"log_token","kind":"source"}"#,
        r#"{"action":"read_file","path":"lib.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"token logging found again",
                "evidence":[{"path":"lib.rs","line":2,"preview":"log_token();"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![
            AgentSpec {
                id: "one".to_string(),
                name: "one".to_string(),
                instruction: "Find token logging.".to_string(),
                model: "gpt-5-nano".to_string(),
                severity: None,
                initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
            },
            AgentSpec {
                id: "two".to_string(),
                name: "two".to_string(),
                instruction: "Find token logging.".to_string(),
                model: "gpt-5-nano".to_string(),
                severity: None,
                initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
            },
        ],
        search.clone(),
        bus,
        1,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts.len(), 2);
    assert!(
        verdicts
            .iter()
            .all(|verdict| verdict.status == TestStatus::Failed)
    );
    assert!(verdicts.iter().all(|verdict| verdict.evidence.len() == 1));
    let stats = search.stats();
    assert_eq!(stats.search_text_misses, 1);
    assert_eq!(stats.search_text_hits, 0);
}

#[tokio::test]
async fn tool_cache_coalesces_concurrent_identical_tool_calls() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = session(temp.path().to_path_buf());
    let cache = ToolExecutionCache::default();

    let (first, second) = tokio::join!(
        async {
            let mut evidence = HashSet::new();
            let executed = execute_tool(
                AgentTurn::SearchText {
                    query: "handler".to_string(),
                    kind: Some("source".to_string()),
                },
                &search,
                &cache,
                &mut evidence,
            )
            .await
            .unwrap();
            (executed.cache_hit, evidence)
        },
        async {
            let mut evidence = HashSet::new();
            let executed = execute_tool(
                AgentTurn::SearchText {
                    query: "handler".to_string(),
                    kind: Some("source".to_string()),
                },
                &search,
                &cache,
                &mut evidence,
            )
            .await
            .unwrap();
            (executed.cache_hit, evidence)
        }
    );

    assert_ne!(first.0, second.0);
    assert!(first.1.contains(&("lib.rs".to_string(), 1)));
    assert!(second.1.contains(&("lib.rs".to_string(), 1)));
    let stats = search.stats();
    assert_eq!(stats.search_text_misses, 1);
    assert_eq!(stats.search_text_hits, 0);
}

struct ScriptedToolBus {
    responses: Mutex<Vec<String>>,
    requests: Mutex<usize>,
}

impl ScriptedToolBus {
    fn new(responses: Vec<&str>) -> Self {
        Self {
            responses: Mutex::new(
                responses
                    .into_iter()
                    .rev()
                    .map(ToString::to_string)
                    .collect(),
            ),
            requests: Mutex::new(0),
        }
    }

    async fn request_count(&self) -> usize {
        *self.requests.lock().await
    }
}

#[async_trait]
impl LlmBus for ScriptedToolBus {
    async fn complete_text(&self, _request: LlmRequest) -> Result<LlmTextResponse, LlmBusError> {
        *self.requests.lock().await += 1;
        let content = self
            .responses
            .lock()
            .await
            .pop()
            .expect("scripted response");
        Ok(LlmTextResponse { content })
    }
}

struct ScriptedActionBus {
    actions: Mutex<Vec<Result<LlmAction, LlmBusError>>>,
    requests: Mutex<usize>,
}

impl ScriptedActionBus {
    fn new(actions: Vec<Result<LlmAction, LlmBusError>>) -> Self {
        Self {
            actions: Mutex::new(actions.into_iter().rev().collect()),
            requests: Mutex::new(0),
        }
    }

    async fn request_count(&self) -> usize {
        *self.requests.lock().await
    }
}

#[async_trait]
impl LlmBus for ScriptedActionBus {
    async fn complete_text(&self, _request: LlmRequest) -> Result<LlmTextResponse, LlmBusError> {
        unreachable!("ScriptedActionBus uses native actions")
    }

    async fn complete_action(&self, _request: LlmRequest) -> Result<LlmAction, LlmBusError> {
        *self.requests.lock().await += 1;
        self.actions.lock().await.pop().expect("scripted action")
    }
}
