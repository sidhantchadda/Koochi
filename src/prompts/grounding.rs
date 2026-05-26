pub fn grounded_agent_prompt(instruction: &str, repository_context: &str) -> String {
    format!(
        r#"Agentic invariant:
{instruction}

Use only the repository context below and tool observations as evidence. Focus on the current review scope, not unrelated pre-existing code. If you fail the invariant, the issue must be in a review-scope file, and evidence entries must use exact repo-relative paths and line numbers from review-scope tool observations. You may inspect other files to understand callers, references, or helpers, but do not report findings whose evidence is only outside the review scope. Do not invent files, line numbers, APIs, or code. If the context is insufficient for a concrete review-scope finding, use the search tools before returning a verdict.

Repository context:
{repository_context}"#
    )
}
