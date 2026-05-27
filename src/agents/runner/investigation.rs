use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ToolKind {
    ReviewCoverage,
    ListFiles,
    ListReviewHunks,
    GetHunkContext,
    SearchText,
    ReadFile,
    GetFileContext,
    FindDefinitions,
    FindReferences,
}

#[derive(Debug)]
pub(super) struct InvestigationState {
    observed: HashSet<ToolKind>,
    require_content: bool,
}

impl InvestigationState {
    pub(super) fn new(agent: &AgentSpec) -> Self {
        let lower_instruction = agent.instruction.to_ascii_lowercase();
        Self {
            observed: HashSet::new(),
            require_content: is_code_review_instruction(&lower_instruction),
        }
    }

    pub(super) fn record(&mut self, kind: ToolKind, _observation: &str) {
        self.observed.insert(kind);
    }

    pub(super) fn final_guidance(&self, response: &LlmResponse) -> Option<String> {
        if response.status == TestStatus::Failed {
            if !self.has_content_observation() {
                return Some(
                    "Failed verdicts require targeted content inspection first. Use get_hunk_context for the most relevant changed hunk, get_file_context for a specific review-scope line, or read_file for the review-scope file, then return failed only if that concrete content demonstrates the issue. list_review_hunks, list_files, and search_text do not satisfy this failed-verdict grounding requirement.".to_string(),
                );
            }
            return None;
        }

        if let Some(guidance) = self.missing_tool_guidance() {
            return Some(guidance);
        }

        None
    }

    pub(super) fn has_content_observation(&self) -> bool {
        self.observed.contains(&ToolKind::ReadFile)
            || self.observed.contains(&ToolKind::GetHunkContext)
            || self.observed.contains(&ToolKind::GetFileContext)
            || self.observed.contains(&ToolKind::ReviewCoverage)
    }

    pub(super) fn missing_tool_guidance(&self) -> Option<String> {
        let mut missing = Vec::new();
        if self.require_content && !self.has_content_observation() {
            missing.push("get_hunk_context, read_file, or get_file_context");
        }
        if missing.is_empty() {
            return None;
        }

        Some(format!(
            "This code-review agentic invariant requires more investigation before verdict. Missing required tool family: {}.",
            missing.join(", ")
        ))
    }
}

fn is_code_review_instruction(instruction: &str) -> bool {
    [
        "verify",
        "do not",
        "fail if",
        "review",
        "check",
        "find",
        "concrete evidence",
    ]
    .iter()
    .any(|needle| instruction.contains(needle))
}
