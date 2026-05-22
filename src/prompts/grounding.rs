pub fn grounded_agent_prompt(instruction: &str, repository_context: &str) -> String {
    format!(
        r#"Agentic test:
{instruction}

Use only the repository context below as evidence. If you fail the test, evidence entries must use exact repo-relative paths and line numbers from the context. Do not invent files, line numbers, APIs, or code. If the context is insufficient for a concrete finding, return passed or failed with an empty evidence array and explain why.

Repository context:
{repository_context}"#
    )
}
