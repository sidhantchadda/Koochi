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
    session_with_mode(root, ReviewMode::FullRepoFallback)
}

fn session_with_mode(root: PathBuf, mode: ReviewMode) -> LocalSearchSession {
    LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root,
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode,
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

#[tokio::test]
async fn runs_agents_through_bus() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
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
    assert_eq!(bus.requests().await.len(), 2);
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
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
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
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await.unwrap());
    let verdicts = run_agents_with_inventory_and_progress(
        agents,
        search,
        bus.clone(),
        2,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
        inventory,
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
    assert_eq!(bus.requests().await.len(), 10);
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
async fn full_repo_prompt_maps_changed_wording_to_repo_code() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn risky_call() {}\n").unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepo,
    ));
    let bus = Arc::new(FakeLlmBus::new());
    run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Fail if changed code can call risky_call.".to_string(),
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
    assert!(request.instruction.contains("Full-repo mode is active"));
    assert!(request.instruction.contains("There is no changed diff"));
    assert!(
        request
            .instruction
            .contains("interpret that as \"review-scope repository code\"")
    );
    assert!(
        request
            .instruction
            .contains("do not return passed merely because no diff exists")
    );
    assert!(
        request
            .instruction
            .contains("delivered coverage chunk that demonstrates the invariant violation")
    );
}

#[tokio::test]
async fn full_repo_prompt_includes_exact_backticked_target_line() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn helper() {}\npub fn dangerous_target() { risky_call(); }\n",
    )
    .unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepoFallback,
    ));
    let bus = Arc::new(FakeLlmBus::new());
    run_agents(
        vec![AgentSpec {
            id: "dangerous-target".to_string(),
            name: "dangerous-target".to_string(),
            instruction: "Inspect `dangerous_target`. Fail if its body calls `risky_call`."
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

    let request = bus.requests().await.remove(0);
    assert!(request.instruction.contains("Full-repo mode is active"));
    assert!(
        request
            .instruction
            .contains("Exact target symbol line for get_file_context: lib.rs:2")
    );
}

#[tokio::test]
async fn full_repo_inventory_reads_all_source_files_before_first_llm_step() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn first() {}\n").unwrap();
    std::fs::write(temp.path().join("app.ts"), "export function second() {}\n").unwrap();
    std::fs::write(temp.path().join("README.md"), "# docs\n").unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepo,
    ));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"final","status":"passed","severity":null,"description":"covered","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"covered after review","evidence":[]}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Pass this invariant.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search.clone(),
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 2);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    let stats = search.stats();
    assert_eq!(stats.read_file_misses, 2);
    assert_eq!(stats.read_file_hits, 0);
}

#[tokio::test]
async fn review_scope_inventory_chunks_source_lines() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));

    let inventory = build_review_scope_inventory(search.as_ref()).await.unwrap();

    assert_eq!(inventory.file_count(), 1);
    assert_eq!(inventory.line_count(), 1);
    assert_eq!(inventory.chunk_count(), 1);
}

#[tokio::test]
async fn commit_mode_inventory_uses_changed_source_hunks_without_reading_files() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("changed.rs"), "pub fn changed() {}\n").unwrap();
    std::fs::write(temp.path().join("unchanged.rs"), "pub fn unchanged() {}\n").unwrap();
    std::fs::write(temp.path().join("notes.md"), "# notes\n").unwrap();
    let search = Arc::new(LocalSearchSession::new(ScopeConfig {
        primary_repo: RepoScope {
            repo_id: "x".to_string(),
            root: temp.path().to_path_buf(),
            revision: GitRevision::Head,
        },
        review: ReviewScope {
            mode: ReviewMode::HeadCommit,
            files: vec!["changed.rs".to_string(), "notes.md".to_string()],
            hunks: vec![
                ReviewHunk {
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
                },
                ReviewHunk {
                    id: "notes.md#1".to_string(),
                    path: "notes.md".to_string(),
                    old_start: 0,
                    old_lines: 0,
                    new_start: 1,
                    new_lines: 1,
                    lines: vec![ReviewHunkLine {
                        kind: ReviewLineKind::Added,
                        old_line: None,
                        new_line: Some(1),
                        content: "# notes".to_string(),
                    }],
                },
            ],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"final","status":"passed","severity":null,"description":"covered","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"covered after review","evidence":[]}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Pass this invariant.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search.clone(),
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Passed);
    let stats = search.stats();
    assert_eq!(stats.read_file_misses, 0);
    assert_eq!(stats.read_file_hits, 0);
}

#[tokio::test]
async fn review_scope_inventory_is_shared_across_agents() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn first() {}\n").unwrap();
    std::fs::write(temp.path().join("main.ts"), "export const second = true;\n").unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepo,
    ));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"final","status":"passed","severity":null,"description":"covered one","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"covered one after review","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"covered two","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"covered two after review","evidence":[]}"#,
    ]));

    let verdicts = run_agents(
        vec![
            AgentSpec {
                id: "one".to_string(),
                name: "one".to_string(),
                instruction: "Pass this invariant.".to_string(),
                model: "gpt-5-nano".to_string(),
                severity: None,
                initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
            },
            AgentSpec {
                id: "two".to_string(),
                name: "two".to_string(),
                instruction: "Pass this invariant.".to_string(),
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
            .all(|verdict| verdict.status == TestStatus::Passed)
    );
    let stats = search.stats();
    assert_eq!(stats.read_file_misses, 2);
    assert_eq!(stats.read_file_hits, 0);
}

#[tokio::test]
async fn pass_requires_review_scope_coverage_without_focus_term_searches() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("cache.ts"),
        "export function cachePrivateLeak() {\n  return 'private cache leak checked'\n}\n",
    )
    .unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepo,
    ));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"final","status":"passed","severity":null,"description":"no finding","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"no finding","evidence":[]}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "private-cache-leak".to_string(),
            name: "private-cache-leak".to_string(),
            instruction: "Fail if changed code can leak private cache data.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search.clone(),
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(bus.request_count().await, 2);
    assert_eq!(search.stats().search_text_misses, 0);
}

#[tokio::test]
async fn pass_waits_until_all_review_scope_coverage_batches_are_delivered() {
    let temp = tempfile::tempdir().unwrap();
    let long_line = "x".repeat(160);
    let content = (0..121)
        .map(|index| format!("pub fn generated_{index}() {{ \"{long_line}\"; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(temp.path().join("large.rs"), content).unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepo,
    ));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"final","status":"passed","severity":null,"description":"first pass","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"second pass","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"covered all chunks","evidence":[]}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "multi-chunk".to_string(),
            name: "multi-chunk".to_string(),
            instruction: "Pass only after review.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search.clone(),
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(verdicts[0].description, "covered all chunks");
    assert_eq!(bus.request_count().await, 3);
    let inventory = build_review_scope_inventory(search.as_ref()).await.unwrap();
    assert_eq!(inventory.chunk_count(), 2);
}

#[tokio::test]
async fn default_diagnostics_do_not_emit_agent_debug_stats() {
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
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await.unwrap());
    let bus = Arc::new(ScriptedActionBus::new(vec![
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "first pass".to_string(),
            evidence: Vec::new(),
        })),
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "covered".to_string(),
            evidence: Vec::new(),
        })),
    ]));
    let mut debug_stats = Vec::new();

    let verdicts = run_agents_with_inventory_and_progress_and_diagnostics(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Pass this invariant.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
        inventory,
        AgentDiagnostics::default(),
        |event| {
            if let AgentProgressEvent::AgentCompleted {
                debug_stats: stats, ..
            } = event
            {
                debug_stats.push(stats);
            }
        },
    )
    .await
    .unwrap();

    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(debug_stats, vec![None]);
}

#[tokio::test]
async fn debug_diagnostics_emit_agent_stats_and_dedupe_unique_loc() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("changed.rs"),
        "pub fn changed() {}\npub fn helper() {}\npub fn third() {}\n",
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
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await.unwrap());
    let bus = Arc::new(ScriptedActionBus::new(vec![
        Ok(LlmAction::Tool(LlmToolCall::ReadFile {
            path: "changed.rs".to_string(),
        })),
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "first pass".to_string(),
            evidence: Vec::new(),
        })),
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "covered".to_string(),
            evidence: Vec::new(),
        })),
    ]));
    let mut debug_stats = Vec::new();

    let verdicts = run_agents_with_inventory_and_progress_and_diagnostics(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Pass this invariant.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
        inventory,
        AgentDiagnostics::default().with_debug_analytics(true),
        |event| {
            if let AgentProgressEvent::AgentCompleted {
                debug_stats: stats, ..
            } = event
            {
                debug_stats.push(stats);
            }
        },
    )
    .await
    .unwrap();

    let stats = debug_stats.pop().flatten().expect("debug stats");
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(bus.request_count().await, 3);
    assert_eq!(stats.test_id, "one");
    assert_eq!(stats.status, TestStatus::Passed);
    assert_eq!(stats.llm_calls, 3);
    assert_eq!(stats.native_tool_calls, 1);
    assert_eq!(stats.native_final_calls, 2);
    assert_eq!(stats.coverage_chunks_delivered, 1);
    assert_eq!(stats.coverage_pass_rejections, 1);
    assert_eq!(stats.unique_loc_read, 3);
    assert_eq!(stats.review_scope_loc, 1);
    assert_eq!(stats.tool_counts.get("read_file"), Some(&1));
}

#[tokio::test]
async fn debug_stats_report_validated_status_after_failed_verdict_downgrade() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("changed.rs"), "pub fn safe() {}\n").unwrap();
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
                    content: "pub fn safe() {}".to_string(),
                }],
            }],
            commit: None,
        },
        accessible_repos: Vec::new(),
        mcp_servers: Vec::new(),
        tools: Vec::new(),
        agents: Vec::new(),
    }));
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await.unwrap());
    let bus = Arc::new(ScriptedActionBus::new(vec![
        Ok(LlmAction::Tool(LlmToolCall::ReadFile {
            path: "changed.rs".to_string(),
        })),
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Failed,
            severity: Some(Severity::High),
            description: "claimed issue with no accepted evidence".to_string(),
            evidence: vec![Evidence {
                path: "changed.rs".to_string(),
                line: 99,
                preview: "pub fn dangerous() {}".to_string(),
            }],
        })),
    ]));
    let mut debug_stats = Vec::new();

    let verdicts = run_agents_with_inventory_and_progress_and_diagnostics(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Fail if code calls dangerous with concrete evidence.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
        inventory,
        AgentDiagnostics::default().with_debug_analytics(true),
        |event| {
            if let AgentProgressEvent::AgentCompleted {
                debug_stats: stats, ..
            } = event
            {
                debug_stats.push(stats);
            }
        },
    )
    .await
    .unwrap();

    let stats = debug_stats.pop().flatten().expect("debug stats");
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(stats.status, TestStatus::Passed);
    assert!(verdicts[0].description.contains("No changed or causal"));
}

#[tokio::test]
async fn repeated_broad_tools_after_coverage_terminate_without_spinning() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn safe() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await.unwrap());
    let bus = Arc::new(ScriptedActionBus::new(vec![
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "first pass".to_string(),
            evidence: Vec::new(),
        })),
        Ok(LlmAction::Tool(LlmToolCall::ListFiles {
            kind: Some("source".to_string()),
        })),
        Ok(LlmAction::Tool(LlmToolCall::ListFiles {
            kind: Some("source".to_string()),
        })),
        Ok(LlmAction::Tool(LlmToolCall::ListFiles {
            kind: Some("source".to_string()),
        })),
    ]));
    let mut debug_stats = Vec::new();

    let verdicts = run_agents_with_inventory_and_progress_and_diagnostics(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Pass this invariant after review.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
        inventory,
        AgentDiagnostics::default().with_debug_analytics(true),
        |event| {
            if let AgentProgressEvent::AgentCompleted {
                debug_stats: stats, ..
            } = event
            {
                debug_stats.push(stats);
            }
        },
    )
    .await
    .unwrap();

    let stats = debug_stats.pop().flatten().expect("debug stats");
    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(
        verdicts[0]
            .description
            .contains("Repeated non-progress tool use")
    );
    assert_eq!(stats.non_progress_terminations, 1);
    assert_eq!(stats.coverage_chunks_delivered, 1);
}

#[tokio::test]
async fn failed_verdict_can_return_before_full_review_scope_coverage() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn risky() {\n    dangerous_sink();\n}\n",
    )
    .unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepo,
    ));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"get_file_context","path":"lib.rs","line":2}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"dangerous sink is reachable",
                "evidence":[{"path":"lib.rs","line":2,"preview":"dangerous_sink();"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "fail-fast".to_string(),
            name: "fail-fast".to_string(),
            instruction: "Fail if code calls dangerous_sink.".to_string(),
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

    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(bus.request_count().await, 3);
}

#[tokio::test]
async fn full_repo_broad_listing_loop_gets_targeted_search_rescue() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn check() {\n    unsafe_origin();\n}\n",
    )
    .unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepo,
    ));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"list_files","kind":"source"}"#,
        r#"{"action":"list_files","kind":"source"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"unsafe origin handling found",
                "evidence":[{"path":"lib.rs","line":2,"preview":"unsafe_origin();"}]
            }"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "origin-validation".to_string(),
            name: "origin-validation".to_string(),
            instruction: "Fail if changed code allows unsafe origin handling.".to_string(),
            model: "gpt-5-nano".to_string(),
            severity: Some(Severity::High),
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search.clone(),
        bus.clone(),
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
    )
    .await
    .unwrap();

    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
    let stats = search.stats();
    assert!(stats.search_text_misses > 0);
    assert!(stats.get_file_context_calls > 0);
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
    assert!(request.instruction.contains("lexical body only"));
    assert!(
        request
            .instruction
            .contains("absence of `Y` is the violation")
    );
    assert!(request.instruction.contains("hunk_id=lib.rs#1 lib.rs:1"));
    assert!(
        request
            .instruction
            .contains("Do not return `failed` from this packet alone")
    );
}

#[tokio::test]
async fn fail_prefixed_prompt_does_not_inject_fixture_answer_keys() {
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
    assert!(!request.instruction.contains("answer key"));
}

#[tokio::test]
async fn rejects_pass_until_review_scope_coverage_is_delivered() {
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
    let bus = Arc::new(ScriptedActionBus::new(vec![
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "changed line is safe".to_string(),
            evidence: Vec::new(),
        })),
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "changed file reviewed and safe".to_string(),
            evidence: Vec::new(),
        })),
    ]));

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

    assert_eq!(bus.request_count().await, 2);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(verdicts[0].description, "changed file reviewed and safe");
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

    assert_eq!(bus.request_count().await, 4);
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
        Ok(LlmAction::Final(LlmResponse {
            status: TestStatus::Passed,
            severity: None,
            description: "no focused changed evidence after full review".to_string(),
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

    assert_eq!(bus.request_count().await, 3);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(
        verdicts[0].description,
        "no focused changed evidence after full review"
    );
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
        r#"{"action":"final","status":"passed","severity":null,"description":"no concrete finding after coverage","evidence":[]}"#,
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

    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert!(
        verdicts[0]
            .description
            .contains("no concrete finding after coverage")
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

    assert_eq!(bus.request_count().await, 7);
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

    assert_eq!(bus.request_count().await, 5);
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

    assert_eq!(bus.request_count().await, 5);
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

    assert_eq!(bus.request_count().await, 4);
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
async fn failed_verdict_rejected_by_adjudicator_continues_to_pass_after_coverage() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn trace_asset() {\n    let userland_module = entry_module();\n}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"get_file_context","path":"lib.rs","line":2}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"Private request data is stored in a shared cache through userland_module.",
                "evidence":[{"path":"lib.rs","line":2,"preview":"let userland_module = entry_module();"}]
            }"#,
        r#"{"decision":"reject_failure","guidance":"The cited line is a module entry reference, not private request data entering a shared cache."}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"No private data cache violation remains after coverage.","evidence":[]}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "private-data-not-static-cached".to_string(),
            name: "private-data-not-static-cached".to_string(),
            instruction: "Fail if private request data is stored in a static or shared cache."
                .to_string(),
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

    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
}

#[tokio::test]
async fn rejected_failure_claim_cannot_be_retried_with_same_evidence_bundle() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn trace_asset() {\n    let userland_module = entry_module();\n}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let repeated_failure = r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"The userland module is private data cached globally.",
                "evidence":[{"path":"lib.rs","line":2,"preview":"let userland_module = entry_module();"}]
            }"#;
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"get_file_context","path":"lib.rs","line":2}"#,
        repeated_failure,
        r#"{"decision":"reject_failure","guidance":"The evidence does not prove private data enters a shared cache."}"#,
        repeated_failure,
        r#"{"action":"final","status":"passed","severity":null,"description":"The repeated assertion was not proven.","evidence":[]}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "private-data-not-static-cached".to_string(),
            name: "private-data-not-static-cached".to_string(),
            instruction: "Fail if private request data is stored in a static or shared cache."
                .to_string(),
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

    assert_eq!(bus.request_count().await, 5);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
}

#[tokio::test]
async fn failed_verdict_can_add_context_after_adjudicator_requests_more_context() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn handler() {\n    let value = request_cookie();\n    shared_cache_store(value);\n}\n",
    )
    .unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"get_file_context","path":"lib.rs","line":3}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"Private request data is stored in a shared cache.",
                "evidence":[{"path":"lib.rs","line":3,"preview":"shared_cache_store(value);"}]
            }"#,
        r#"{"decision":"needs_more_context","guidance":"Show the source of value before accepting the cache sink claim."}"#,
        r#"{"action":"get_file_context","path":"lib.rs","line":2}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"Request cookie data flows into the shared cache store.",
                "evidence":[
                    {"path":"lib.rs","line":2,"preview":"let value = request_cookie();"},
                    {"path":"lib.rs","line":3,"preview":"shared_cache_store(value);"}
                ]
            }"#,
        r#"{"decision":"accept_failure","guidance":"The bundle shows request-derived data and the shared cache sink."}"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "private-data-not-static-cached".to_string(),
            name: "private-data-not-static-cached".to_string(),
            instruction: "Fail if private request data is stored in a static or shared cache."
                .to_string(),
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

    assert_eq!(bus.request_count().await, 6);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 2);
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
async fn failed_verdict_for_named_target_rejects_sibling_evidence() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn danger_target(signal: &Signal) -> i64 { signal.value * 100 }\npub fn sibling(signal: &Signal) -> i64 { signal.value * 100 }\n",
    )
    .unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepoFallback,
    ));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"read_file","path":"lib.rs"}"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"sibling multiplies signal.value directly",
                "evidence":[{"path":"lib.rs","line":2,"preview":"pub fn sibling(signal: &Signal) -> i64 { signal.value * 100 }"}]
            }"#,
        r#"{
                "action":"final",
                "status":"failed",
                "severity":"high",
                "description":"danger_target multiplies signal.value directly",
                "evidence":[{"path":"lib.rs","line":1,"preview":"pub fn danger_target(signal: &Signal) -> i64 { signal.value * 100 }"}]
            }"#,
    ]));

    let verdicts = run_agents(
        vec![AgentSpec {
            id: "danger-target".to_string(),
            name: "danger-target".to_string(),
            instruction:
                "Inspect `danger_target`. Fail if its body contains direct multiplication of `signal.value`."
                    .to_string(),
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

    assert_eq!(bus.request_count().await, 4);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence[0].line, 1);
}

#[test]
fn passed_without_failure_condition_is_repaired_from_target_evidence() {
    let response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "No violation found.".to_string(),
        evidence: vec![Evidence {
            path: "src/gates.rs".to_string(),
            line: 11,
            preview: "pub fn gate_bad_missing_label(request: &GateRequest) -> GateDecision { let mut decision = base_decision(\"gate_bad_missing_label\"); if request.amount < 10_000 { decision.open = true; } decision }".to_string(),
        }],
    };

    assert!(passed_verdict_directly_satisfies_fail_condition(
        &response,
        "Inspect `gate_bad_missing_label`. Fail if its own body assigns `decision.open = true` without checking `label_present(request)` in that same body.",
        &Some(("src/gates.rs".to_string(), 11)),
    ));

    let direct_call_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "No violation found.".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 12,
            preview: "pub fn meter_bad_empty_label(signal: &Signal) -> Option<MeterItem> { if signal.value > 12_000 { Some(meter_item(\"meter_bad_empty_label\", \"red\", format!(\"{} accepted\", signal.name))) } else { None } }".to_string(),
        }],
    };

    assert!(passed_verdict_directly_satisfies_fail_condition(
        &direct_call_response,
        "Inspect `meter_bad_empty_label`. Fail if its body calls `meter_item` directly instead of `meter_ok`.",
        &Some(("src/meter.rs".to_string(), 12)),
    ));

    let direct_zone_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "No violation found.".to_string(),
        evidence: vec![Evidence {
            path: "src/rows.rs".to_string(),
            line: 12,
            preview: "pub fn row_bad_plain_zone(record: &RowRecord, group: &str) -> Option<String> { if !same_group(record, group) || !record.open { return None; } Some(format!(\"{},{}\", record.zone, clean_piece(&record.item))) }".to_string(),
        }],
    };

    assert!(passed_verdict_directly_satisfies_fail_condition(
        &direct_zone_response,
        "Inspect `row_bad_plain_zone`. Fail if its `format!` call uses `record.zone` directly.",
        &Some(("src/rows.rs".to_string(), 12)),
    ));

    let open_limit_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "No violation found.".to_string(),
        evidence: vec![Evidence {
            path: "src/gates.rs".to_string(),
            line: 13,
            preview: "pub fn gate_bad_open_limit(request: &GateRequest) -> GateDecision { let mut decision = base_decision(\"gate_bad_open_limit\"); if label_present(request) && request.stamp_count >= 2 { decision.open = true; } decision }".to_string(),
        }],
    };

    assert!(passed_verdict_directly_satisfies_fail_condition(
        &open_limit_response,
        "Inspect `gate_bad_open_limit`. Fail if it opens without an amount limit comparison.",
        &Some(("src/gates.rs".to_string(), 13)),
    ));

    let row_ok_absence_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description: "No violation found.".to_string(),
        evidence: vec![Evidence {
            path: "src/rows.rs".to_string(),
            line: 11,
            preview: "pub fn row_bad_group_skip(record: &RowRecord, _group: &str) -> Option<String> { if !record.open { return None; } Some(format!(\"{},{}\", clean_piece(&record.zone), clean_piece(&record.item))) }".to_string(),
        }],
    };

    assert!(passed_verdict_directly_satisfies_fail_condition(
        &row_ok_absence_response,
        "Inspect `row_bad_group_skip`. Fail if its own body contains `Some(format!` and does not contain a `row_ok(` call.",
        &Some(("src/rows.rs".to_string(), 11)),
    ));
}

#[test]
fn failed_pass_only_verdict_is_repaired_when_target_line_satisfies_condition() {
    let body_calls_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "meter_rule_002 does not call meter_ok".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 18,
            preview: "pub fn meter_rule_002(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_002\", 6002) }".to_string(),
        }],
    };

    assert!(
        failed_verdict_contradicts_pass_only_target_evidence(
            &body_calls_response,
            "Inspect `meter_rule_002`. Pass only if the body calls `meter_ok`.",
            &Some(("src/meter.rs".to_string(), 18)),
        )
        .is_some()
    );

    let no_if_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "meter_rule_013 lexical body contains an if token".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 29,
            preview: "pub fn meter_rule_013(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_013\", 6013) }".to_string(),
        }],
    };

    assert!(failed_verdict_contradicts_pass_only_target_evidence(
        &no_if_response,
        "Inspect `meter_rule_013`. Pass only if the lexical body of `meter_rule_013` contains no `if` token; ignore helper bodies.",
        &Some(("src/meter.rs".to_string(), 29)),
    )
    .is_some());

    let first_arg_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "first argument is not signal".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 33,
            preview: "pub fn meter_rule_017(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_017\", 6017) }".to_string(),
        }],
    };

    assert!(failed_verdict_contradicts_pass_only_target_evidence(
        &first_arg_response,
        "Inspect `meter_rule_017`. Pass only if the first argument to `meter_ok` is exactly `signal`.",
        &Some(("src/meter.rs".to_string(), 33)),
    )
    .is_some());

    let second_arg_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "second argument is not literal".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 37,
            preview: "pub fn meter_rule_021(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_021\", 6021) }".to_string(),
        }],
    };

    assert!(failed_verdict_contradicts_pass_only_target_evidence(
        &second_arg_response,
        "Inspect `meter_rule_021`. Pass only if the second argument to `meter_ok` is the string literal `meter_rule_021`.",
        &Some(("src/meter.rs".to_string(), 37)),
    )
    .is_some());

    let last_arg_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "last argument is not 6012".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 28,
            preview: "pub fn meter_rule_012(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_012\", 6012) }".to_string(),
        }],
    };

    assert!(
        failed_verdict_contradicts_pass_only_target_evidence(
            &last_arg_response,
            "Inspect `meter_rule_012`. Pass only if the last `meter_ok` argument is `6012`.",
            &Some(("src/meter.rs".to_string(), 28)),
        )
        .is_some()
    );

    let call_target_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "call target is not meter_ok".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 32,
            preview: "pub fn meter_rule_016(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_016\", 6016) }".to_string(),
        }],
    };

    assert!(
        failed_verdict_contradicts_pass_only_target_evidence(
            &call_target_response,
            "Inspect `meter_rule_016`. Pass only if the call target is `meter_ok`.",
            &Some(("src/meter.rs".to_string(), 32)),
        )
        .is_some()
    );

    let direct_result_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "does not return helper directly".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 22,
            preview: "pub fn meter_rule_006(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_006\", 6006) }".to_string(),
        }],
    };

    assert!(failed_verdict_contradicts_pass_only_target_evidence(
        &direct_result_response,
        "Inspect `meter_rule_006`. Pass only if the rule returns the `meter_ok` result directly.",
        &Some(("src/meter.rs".to_string(), 22)),
    )
    .is_some());

    let literal_contains_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "literal lacks 015".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 31,
            preview: "pub fn meter_rule_015(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_015\", 6015) }".to_string(),
        }],
    };

    assert!(
        failed_verdict_contradicts_pass_only_target_evidence(
            &literal_contains_response,
            "Inspect `meter_rule_015`. Pass only if its literal label contains `015`.",
            &Some(("src/meter.rs".to_string(), 31)),
        )
        .is_some()
    );

    let avoids_helper_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "calls bounded_value through helper".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 39,
            preview: "pub fn meter_rule_023(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_023\", 6023) }".to_string(),
        }],
    };

    assert!(
        failed_verdict_contradicts_pass_only_target_evidence(
            &avoids_helper_response,
            "Inspect `meter_rule_023`. Pass only if it avoids calling `bounded_value` itself.",
            &Some(("src/meter.rs".to_string(), 39)),
        )
        .is_some()
    );

    let route_literal_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "literal is wrong".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 20,
            preview: "pub fn meter_rule_004(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_004\", 6004) }".to_string(),
        }],
    };

    assert!(
        failed_verdict_contradicts_pass_only_target_evidence(
            &route_literal_response,
            "Inspect `meter_rule_004`. Pass only if the route name literal is `meter_rule_004`.",
            &Some(("src/meter.rs".to_string(), 20)),
        )
        .is_some()
    );

    let no_local_variable_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "has a local variable".to_string(),
        evidence: vec![Evidence {
            path: "src/meter.rs".to_string(),
            line: 36,
            preview: "pub fn meter_rule_020(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, \"meter_rule_020\", 6020) }".to_string(),
        }],
    };

    assert!(
        failed_verdict_contradicts_pass_only_target_evidence(
            &no_local_variable_response,
            "Inspect `meter_rule_020`. Pass only if it has no local variable before returning.",
            &Some(("src/meter.rs".to_string(), 36)),
        )
        .is_some()
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

    assert_eq!(bus.request_count().await, 4);
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

    assert_eq!(bus.request_count().await, 5);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
}

#[tokio::test]
async fn missing_read_file_tool_result_is_observation_not_run_error() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(session_with_mode(
        temp.path().to_path_buf(),
        ReviewMode::FullRepo,
    ));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"action":"read_file","path":"src/lib.rs"}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"missing example path did not provide evidence","evidence":[]}"#,
        r#"{"action":"final","status":"passed","severity":null,"description":"missing example path did not provide evidence after coverage","evidence":[]}"#,
    ]));
    let verdicts = run_agents(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Check changed code for unsafe behavior.".to_string(),
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

    assert_eq!(bus.request_count().await, 5);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
}

#[tokio::test]
async fn invalid_prompt_provider_error_records_prompt_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let inventory = Arc::new(build_review_scope_inventory(search.as_ref()).await.unwrap());
    let bus = Arc::new(ScriptedActionBus::new(vec![Err(LlmBusError::HttpStatus {
        status: reqwest::StatusCode::BAD_REQUEST,
        body: r#"{"error":{"message":"Invalid prompt","code":"invalid_prompt"}}"#.to_string(),
    })]));
    let dump_dir = temp.path().join(".koochi").join("debug").join("prompts");
    let error = run_agents_with_inventory_and_progress_and_diagnostics(
        vec![AgentSpec {
            id: "one".to_string(),
            name: "one".to_string(),
            instruction: "Find token logging. OPENAI_API_KEY=sk-testsecretvalue1234567890"
                .to_string(),
            model: "gpt-5-nano".to_string(),
            severity: None,
            initial_context_token_budget: crate::config::DEFAULT_INITIAL_CONTEXT_TOKEN_BUDGET,
        }],
        search,
        bus,
        128,
        crate::config::DEFAULT_MAX_AGENT_STEPS,
        inventory,
        AgentDiagnostics::with_prompt_dump_dir(dump_dir.clone()),
        |_| {},
    )
    .await
    .unwrap_err();

    let AgentError::PromptRejected {
        test_id,
        step,
        prompt_tokens,
        prompt_dump_path,
        source,
    } = error
    else {
        panic!("expected prompt rejection diagnostic, got {error:?}");
    };

    assert_eq!(test_id, "one");
    assert_eq!(step, 1);
    assert!(prompt_tokens > 0);
    assert!(prompt_dump_path.starts_with(&dump_dir));
    assert!(matches!(
        source,
        LlmBusError::HttpStatus {
            status: reqwest::StatusCode::BAD_REQUEST,
            ..
        }
    ));
    let dump = std::fs::read_to_string(prompt_dump_path).unwrap();
    assert!(dump.contains(r#""test_id": "one""#));
    assert!(dump.contains(r#""step": 1"#));
    assert!(dump.contains("prompt_redacted"));
    assert!(dump.contains("OPENAI_API_KEY=[REDACTED]"));
    assert!(!dump.contains("sk-testsecretvalue1234567890"));
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
        r#"{"description":"safe route found","severity":"low","evidence":[{"path":"src/workflows.rs","line":189,"preview":"ensure_workflow_route();"}]}"#,
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

    let no_finding_then_failed_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "No concrete violation found. The code shows the fail condition, so the correct verdict should be failed."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &no_finding_then_failed_response,
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

    let no_evidence_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "Invariant not violated in reviewed scope: no evidence of requests being executed without origin/host validation was observed."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &no_evidence_response,
        "Fail if changed Server Action request handling can execute an action without validating the request origin or forwarded host against the configured allowed origins."
    ));

    let no_concrete_evidence_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "No concrete evidence found in the changed review-scope files that draft or preview mode code can render previews without validating signed cookies or secret tokens."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &no_concrete_evidence_response,
        "Fail if changed draft mode or preview mode code can render previews without validating signed cookies or secret tokens."
    ));

    let no_concrete_change_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "No concrete change in the review-scope files indicates that changed draft mode or preview mode code enables preview rendering without validating signed cookies or secret tokens."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &no_concrete_change_response,
        "Fail if changed draft mode or preview mode code can render previews without validating signed cookies or secret tokens."
    ));

    let no_changed_capability_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "No changed draft mode or preview mode code enables preview rendering without validating signed cookies or secret tokens."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &no_changed_capability_response,
        "Fail if changed draft mode or preview mode code can render previews without validating signed cookies or secret tokens."
    ));

    let server_only_not_found_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "Server/client boundary checks observed in build tooling; no client import of server-only APIs detected in reviewed area."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &server_only_not_found_response,
        "Fail if changed bundling, module graph, or import analysis allows modules marked server-only or containing server-only APIs to be imported into client components or browser bundles."
    ));

    let client_only_not_found_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "No client-only modules were found executing in a server context in the reviewed source."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &client_only_not_found_response,
        "Fail if changed bundling, module graph, or runtime code executes client-only modules in a server context where browser globals, side effects, or hydration assumptions can break rendering."
    ));

    let telemetry_not_found_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "No tokens, cookies, or sensitive identifiers were detected in the reviewed telemetry code."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(!passed_verdict_contradicts_failure_language(
        &telemetry_not_found_response,
        "Fail if changed telemetry, analytics, tracing, or metrics code emits raw project paths, usernames, environment variables, tokens, request headers, cookies, or other sensitive identifiers without redaction."
    ));

    let missing_validation_response = LlmResponse {
        status: TestStatus::Passed,
        severity: None,
        description:
            "Origin validation was not observed before executing the Server Action request."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(passed_verdict_contradicts_failure_language(
        &missing_validation_response,
        "Fail if changed Server Action request handling can execute an action without validating the request origin or forwarded host against the configured allowed origins."
    ));
}

#[test]
fn failed_verdict_with_no_finding_or_coverage_language_is_rejected() {
    let no_violation_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description:
            "The reviewed target satisfies the invariant; no concrete violation was found."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(failed_verdict_contradicts_no_finding_language(
        &no_violation_response
    ));

    let coverage_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description:
            "The target appears correct, but I cannot declare passed because coverage is incomplete."
                .to_string(),
        evidence: Vec::new(),
    };

    assert!(failed_verdict_contradicts_no_finding_language(
        &coverage_response
    ));

    let real_failure_response = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "Invariant violation detected: the target opens after one stamp.".to_string(),
        evidence: Vec::new(),
    };

    assert!(!failed_verdict_contradicts_no_finding_language(
        &real_failure_response
    ));
}

#[test]
fn full_repo_failed_verdict_requires_focused_evidence_preview() {
    let weak = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "lockfile trust issue".to_string(),
        evidence: vec![Evidence {
            path: "packages/next/src/build/lockfile.ts".to_string(),
            line: 175,
            preview: "let lockfile".to_string(),
        }],
    };
    let terms = vec![
        "package".to_string(),
        "manager".to_string(),
        "lockfile".to_string(),
        "trust".to_string(),
    ];

    assert!(failed_verdict_lacks_full_repo_focus_evidence(&weak, &terms));

    let focused = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "lockfile trust issue".to_string(),
        evidence: vec![Evidence {
            path: "packages/next/src/build/lockfile.ts".to_string(),
            line: 175,
            preview: "execute package manager command from lockfile metadata".to_string(),
        }],
    };

    assert!(!failed_verdict_lacks_full_repo_focus_evidence(
        &focused, &terms
    ));

    let bundled = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "private cache issue".to_string(),
        evidence: vec![Evidence {
            path: ".github/actions/needs-triage/dist/index.js".to_string(),
            line: 1,
            preview: "(()=>{var __webpack_modules__={9479:function(e,A,t){".to_string(),
        }],
    };

    assert!(failed_verdict_lacks_full_repo_focus_evidence(
        &bundled,
        &[
            "private".to_string(),
            "cache".to_string(),
            "static".to_string()
        ]
    ));

    let config_declaration = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "remote allowlist issue".to_string(),
        evidence: vec![Evidence {
            path: "packages/next/src/server/image-optimizer.ts".to_string(),
            line: 394,
            preview: "const remotePatterns = nextConfig.images?.remotePatterns || []".to_string(),
        }],
    };

    assert!(failed_verdict_lacks_full_repo_focus_evidence(
        &config_declaration,
        &[
            "image".to_string(),
            "remote".to_string(),
            "allowlist".to_string(),
            "arbitrary".to_string()
        ]
    ));
}

#[test]
fn speculative_failed_verdict_is_rejected() {
    let speculative = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description:
            "Invariant violation: a concrete violation would be if this path fetches without validation, but targeted scan is required to confirm enforcement."
                .to_string(),
        evidence: vec![Evidence {
            path: "packages/next/src/server/image-optimizer.ts".to_string(),
            line: 394,
            preview: "const remotePatterns = nextConfig.images?.remotePatterns || []".to_string(),
        }],
    };

    assert!(failed_verdict_is_speculative(&speculative));

    let modal_lockfile = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::Medium),
        description:
            "Invariant violation: review-scope code appears to rely on package manager metadata. This could enable arbitrary commands if metadata is altered at runtime."
                .to_string(),
        evidence: vec![Evidence {
            path: ".github/actions/next-stats-action/src/run/index.js".to_string(),
            line: 338,
            preview:
                "await exec(`cd ${pkgDir} && pnpm install --strict-peer-dependencies=false`, false)"
                    .to_string(),
        }],
    };

    assert!(failed_verdict_is_speculative(&modal_lockfile));

    let needs_more_evidence = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description:
            "Invariant violation: no clear enforcement is shown here; targeted evidence required from review-scope code demonstrating an absence or bypass of allowlist checks."
                .to_string(),
        evidence: vec![Evidence {
            path: "packages/next/src/server/image-optimizer.ts".to_string(),
            line: 394,
            preview: "const remotePatterns = nextConfig.images?.remotePatterns || []".to_string(),
        }],
    };

    assert!(failed_verdict_is_speculative(&needs_more_evidence));

    let concrete = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description:
            "The fetch path returns before checking the configured allowlist, so arbitrary remote hosts are accepted."
                .to_string(),
        evidence: vec![Evidence {
            path: "packages/next/src/server/image-optimizer.ts".to_string(),
            line: 401,
            preview: "return fetch(url)".to_string(),
        }],
    };

    assert!(!failed_verdict_is_speculative(&concrete));
}

#[tokio::test]
async fn failed_verdict_requires_preview_to_match_actual_source_line() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "pub fn render() {\n    return make_network_error();\n}\n",
    )
    .unwrap();
    let search = session(temp.path().to_path_buf());
    let evidence_index = HashSet::from([("src/lib.rs".to_string(), 2)]);
    let review_paths = HashSet::new();
    let changed_lines = HashSet::new();
    let relevant_changed_lines = HashSet::new();

    let fabricated = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "status boundary issue".to_string(),
        evidence: vec![Evidence {
            path: "src/lib.rs".to_string(),
            line: 2,
            preview: "end of interrupted render path; may not set proper status".to_string(),
        }],
    };

    assert!(
        failed_verdict_has_mismatched_evidence_preview(
            &search,
            &fabricated,
            &evidence_index,
            &review_paths,
            &changed_lines,
            &relevant_changed_lines
        )
        .await
        .unwrap()
        .is_some()
    );

    let exact = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "status boundary issue".to_string(),
        evidence: vec![Evidence {
            path: "src/lib.rs".to_string(),
            line: 2,
            preview: "return make_network_error();".to_string(),
        }],
    };

    assert!(
        failed_verdict_has_mismatched_evidence_preview(
            &search,
            &exact,
            &evidence_index,
            &review_paths,
            &changed_lines,
            &relevant_changed_lines
        )
        .await
        .unwrap()
        .is_none()
    );

    let line_numbered_preview = LlmResponse {
        status: TestStatus::Failed,
        severity: Some(Severity::High),
        description: "status boundary issue".to_string(),
        evidence: vec![Evidence {
            path: "src/lib.rs".to_string(),
            line: 2,
            preview: "2: return make_network_error();".to_string(),
        }],
    };

    assert!(
        failed_verdict_has_mismatched_evidence_preview(
            &search,
            &line_numbered_preview,
            &evidence_index,
            &review_paths,
            &changed_lines,
            &relevant_changed_lines
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn accepts_plain_verdict_json_as_final_turn() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("lib.rs"), "pub fn handler() {}\n").unwrap();
    let search = Arc::new(session(temp.path().to_path_buf()));
    let bus = Arc::new(ScriptedToolBus::new(vec![
        r#"{"status":"passed","severity":null,"description":"No secrets found.","evidence":[]}"#,
        r#"{"status":"passed","severity":null,"description":"No secrets found after review.","evidence":[]}"#,
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

    assert_eq!(bus.request_count().await, 2);
    assert_eq!(verdicts[0].status, TestStatus::Passed);
    assert_eq!(verdicts[0].description, "No secrets found after review.");
}

#[tokio::test]
async fn fail_prefixed_real_invariant_does_not_require_answer_key_search() {
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

    assert_eq!(bus.request_count().await, 3);
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert_eq!(verdicts[0].evidence.len(), 1);
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
    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert!(verdicts[0].description.contains("step limit"));
    assert!(verdicts[0].description.contains("Passing is not allowed"));
}

#[tokio::test]
async fn step_limit_without_full_coverage_cannot_pass_even_with_concrete_evidence_instruction() {
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

    assert_eq!(verdicts[0].status, TestStatus::Failed);
    assert!(verdicts[0].description.contains("step limit"));
    assert!(verdicts[0].description.contains("Passing is not allowed"));
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
                false,
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
                false,
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

#[tokio::test]
async fn execute_tool_collects_shown_source_lines_only_when_debug_requested() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn first() {}\npub fn second() {}\n",
    )
    .unwrap();
    let search = session(temp.path().to_path_buf());
    let mut evidence = HashSet::new();
    let normal_cache = ToolExecutionCache::default();
    let normal = execute_tool(
        AgentTurn::ReadFile {
            path: "lib.rs".to_string(),
        },
        &search,
        &normal_cache,
        &mut evidence,
        false,
    )
    .await
    .unwrap();

    assert!(normal.shown_source_lines.is_empty());
    assert!(evidence.contains(&("lib.rs".to_string(), 1)));
    assert!(evidence.contains(&("lib.rs".to_string(), 2)));

    let mut debug_evidence = HashSet::new();
    let debug_cache = ToolExecutionCache::default();
    let debug = execute_tool(
        AgentTurn::ReadFile {
            path: "lib.rs".to_string(),
        },
        &search,
        &debug_cache,
        &mut debug_evidence,
        true,
    )
    .await
    .unwrap();

    assert_eq!(
        debug.shown_source_lines,
        vec![("lib.rs".to_string(), 1), ("lib.rs".to_string(), 2)]
    );
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
    async fn complete_text(&self, request: LlmRequest) -> Result<LlmTextResponse, LlmBusError> {
        *self.requests.lock().await += 1;
        let is_adjudication = request
            .instruction
            .contains("Failure adjudication for Koochi invariant");
        let content = if is_adjudication {
            let mut responses = self.responses.lock().await;
            if responses
                .last()
                .is_some_and(|response| response.contains(r#""decision""#))
            {
                responses.pop().unwrap()
            } else {
                r#"{"decision":"accept_failure","guidance":"test adjudicator accepts"}"#.to_string()
            }
        } else if let Some(content) = self.responses.lock().await.pop() {
            content
        } else {
            panic!("scripted response")
        };
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
    async fn complete_text(&self, request: LlmRequest) -> Result<LlmTextResponse, LlmBusError> {
        *self.requests.lock().await += 1;
        if request
            .instruction
            .contains("Failure adjudication for Koochi invariant")
        {
            return Ok(LlmTextResponse {
                content: r#"{"decision":"accept_failure","guidance":"test adjudicator accepts"}"#
                    .to_string(),
            });
        }
        unreachable!("ScriptedActionBus uses native actions")
    }

    async fn complete_action(&self, _request: LlmRequest) -> Result<LlmAction, LlmBusError> {
        *self.requests.lock().await += 1;
        self.actions.lock().await.pop().expect("scripted action")
    }
}
