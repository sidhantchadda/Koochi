use super::verdict::AgentVerdict;
use crate::llm::LlmBus;
use crate::llm::LlmBusError;
use crate::llm::LlmRequest;
use crate::prompts::grounded_agent_prompt;
use crate::scope::AgentSpec;
use crate::search::CodeSearchApi;
use crate::search::FileKind;
use crate::search::ListFilesRequest;
use crate::search::ReadFileRequest;
use std::collections::HashSet;
use std::fmt::Display;
use std::sync::Arc;

const MAX_CONTEXT_FILES: usize = 32;
const MAX_CONTEXT_LINES_PER_FILE: usize = 200;
const MAX_CONTEXT_BYTES: usize = 48_000;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Llm(#[from] LlmBusError),
    #[error("search failed while preparing agent context: {0}")]
    Search(String),
    #[error("agent task failed: {0}")]
    Join(tokio::task::JoinError),
}

pub async fn run_agents<S, B>(
    agents: Vec<AgentSpec>,
    search: Arc<S>,
    bus: Arc<B>,
    max_parallel_agents: usize,
) -> Result<Vec<AgentVerdict>, AgentError>
where
    S: CodeSearchApi + 'static,
    S::Error: Display,
    B: LlmBus + ?Sized + 'static,
{
    let mut verdicts = Vec::new();
    for chunk in agents.chunks(max_parallel_agents.max(1)) {
        let mut requests = Vec::with_capacity(chunk.len());
        let mut evidence_indexes = Vec::with_capacity(chunk.len());
        for agent in chunk {
            let grounded = build_grounded_request(agent, search.as_ref()).await?;
            evidence_indexes.push(grounded.evidence_index);
            requests.push(grounded.request);
        }
        let responses = bus.complete_batch(requests).await?;
        for ((agent, response), evidence_index) in chunk.iter().zip(responses).zip(evidence_indexes)
        {
            verdicts.push(AgentVerdict {
                test_id: agent.id.clone(),
                status: response.status,
                severity: response.severity.or(agent.severity),
                description: response.description,
                evidence: response
                    .evidence
                    .into_iter()
                    .filter(|evidence| {
                        evidence_index.contains(&(evidence.path.clone(), evidence.line))
                    })
                    .collect(),
            });
        }
    }
    Ok(verdicts)
}

struct GroundedRequest {
    request: LlmRequest,
    evidence_index: HashSet<(String, u32)>,
}

async fn build_grounded_request<S>(
    agent: &AgentSpec,
    search: &S,
) -> Result<GroundedRequest, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    let files = search
        .list_files(ListFilesRequest {
            kind: FileKind::Source,
        })
        .await
        .map_err(|err| AgentError::Search(err.to_string()))?
        .files;

    let mut context = String::new();
    let mut evidence_index = HashSet::new();
    for path in files.into_iter().take(MAX_CONTEXT_FILES) {
        let file = search
            .read_file(ReadFileRequest { path: path.clone() })
            .await
            .map_err(|err| AgentError::Search(err.to_string()))?;
        if context.len() >= MAX_CONTEXT_BYTES {
            break;
        }
        context.push_str(&format!("\n--- file: {} ---\n", file.path));
        for (index, line) in file
            .content
            .lines()
            .take(MAX_CONTEXT_LINES_PER_FILE)
            .enumerate()
        {
            let line_number = (index + 1) as u32;
            evidence_index.insert((file.path.clone(), line_number));
            context.push_str(&format!("{line_number}: {line}\n"));
            if context.len() >= MAX_CONTEXT_BYTES {
                break;
            }
        }
    }

    let instruction = grounded_agent_prompt(&agent.instruction, context.trim());

    Ok(GroundedRequest {
        request: LlmRequest {
            test_id: agent.id.clone(),
            model: agent.model.clone(),
            instruction,
        },
        evidence_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;
    use crate::llm::Evidence;
    use crate::llm::FakeLlmBus;
    use crate::llm::LlmResponse;
    use crate::llm::TestStatus;
    use crate::scope::{GitRevision, RepoScope, ScopeConfig};
    use crate::search::LocalSearchSession;
    use async_trait::async_trait;
    use std::path::PathBuf;

    fn session(root: PathBuf) -> LocalSearchSession {
        LocalSearchSession::new(ScopeConfig {
            primary_repo: RepoScope {
                repo_id: "x".to_string(),
                root,
                revision: GitRevision::Head,
            },
            accessible_repos: Vec::new(),
            mcp_servers: Vec::new(),
            tools: Vec::new(),
            agents: Vec::new(),
        })
    }

    #[tokio::test]
    async fn runs_agents_through_bus() {
        let search = Arc::new(LocalSearchSession::new(ScopeConfig {
            primary_repo: RepoScope {
                repo_id: "x".to_string(),
                root: PathBuf::from("."),
                revision: GitRevision::Head,
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
                instruction: "Pass this".to_string(),
                model: "gpt-5.4-nano".to_string(),
                severity: None,
            }],
            search,
            bus.clone(),
            128,
        )
        .await
        .unwrap();
        assert_eq!(verdicts.len(), 1);
        assert_eq!(bus.requests().await.len(), 1);
        assert_eq!(bus.batches().await, vec![1]);
    }

    #[tokio::test]
    async fn batches_agents_by_configured_limit() {
        let search = Arc::new(LocalSearchSession::new(ScopeConfig {
            primary_repo: RepoScope {
                repo_id: "x".to_string(),
                root: PathBuf::from("."),
                revision: GitRevision::Head,
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
                instruction: "Pass this".to_string(),
                model: "gpt-5.4-nano".to_string(),
                severity: None,
            })
            .collect();
        let verdicts = run_agents(agents, search, bus.clone(), 2).await.unwrap();
        assert_eq!(verdicts.len(), 5);
        assert_eq!(bus.batches().await, vec![2, 2, 1]);
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
                model: "gpt-5.4-nano".to_string(),
                severity: None,
            }],
            search,
            bus.clone(),
            128,
        )
        .await
        .unwrap();
        let request = bus.requests().await.remove(0);
        assert!(request.instruction.contains("--- file: lib.rs ---"));
        assert!(request.instruction.contains("1: pub fn risky_call() {}"));
    }

    #[tokio::test]
    async fn drops_provider_evidence_not_found_in_repo_context() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("lib.rs"), "pub fn risky_call() {}\n").unwrap();
        let search = Arc::new(session(temp.path().to_path_buf()));
        let bus = Arc::new(HallucinatingBus);
        let verdicts = run_agents(
            vec![AgentSpec {
                id: "one".to_string(),
                name: "one".to_string(),
                instruction: "missing auth".to_string(),
                model: "gpt-5.4-nano".to_string(),
                severity: None,
            }],
            search,
            bus,
            128,
        )
        .await
        .unwrap();
        assert_eq!(verdicts[0].status, TestStatus::Failed);
        assert_eq!(verdicts[0].evidence.len(), 1);
        assert_eq!(verdicts[0].evidence[0].path, "lib.rs");
    }

    struct HallucinatingBus;

    #[async_trait]
    impl LlmBus for HallucinatingBus {
        async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse, LlmBusError> {
            Ok(LlmResponse {
                status: TestStatus::Failed,
                severity: Some(Severity::High),
                description: "failed".to_string(),
                evidence: vec![
                    Evidence {
                        path: "lib.rs".to_string(),
                        line: 1,
                        preview: "pub fn risky_call() {}".to_string(),
                    },
                    Evidence {
                        path: "/made/up.js".to_string(),
                        line: 42,
                        preview: "nope".to_string(),
                    },
                ],
            })
        }
    }
}
