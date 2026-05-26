use super::*;

pub(super) fn verdict_from_loop_result(
    agent: &AgentSpec,
    loop_result: AgentLoopResult,
) -> AgentVerdict {
    let elapsed_ms = loop_result.elapsed.as_millis();
    let response = loop_result.response;
    let evidence_index = loop_result.evidence_index;
    let review_paths = loop_result.review_paths;
    let changed_lines = loop_result.changed_lines;
    let relevant_changed_lines = loop_result.relevant_changed_lines;
    let review_causal_terms = loop_result.review_causal_terms;
    let classifications = classify_evidence(
        &response.evidence,
        &evidence_index,
        &review_paths,
        &changed_lines,
        &relevant_changed_lines,
    );
    let has_changed_evidence = classifications
        .iter()
        .any(|item| item.classification == EvidenceClassification::Changed);
    let has_review_context_evidence = classifications
        .iter()
        .any(|item| item.classification == EvidenceClassification::ReviewContext);
    let has_causal_review_context = has_review_context_evidence
        && (changed_lines.is_empty()
            || response_references_changed_context(&response, &review_causal_terms));
    let has_accepted_failure_evidence = has_changed_evidence || has_causal_review_context;
    let response_status = response.status;
    let response_severity = response.severity;
    let response_description = response.description;
    let accepted_evidence = response
        .evidence
        .into_iter()
        .filter(|evidence| {
            classify_single_evidence(
                evidence,
                &evidence_index,
                &review_paths,
                &changed_lines,
                &relevant_changed_lines,
            )
            .is_some_and(|classification| {
                matches!(
                    classification,
                    EvidenceClassification::Changed | EvidenceClassification::ReviewContext
                )
            })
        })
        .collect::<Vec<_>>();
    let (status, description) = if response_status == TestStatus::Failed
        && !has_accepted_failure_evidence
        && !is_infrastructure_failure(&response_description)
        && !is_absence_policy(&agent.instruction)
    {
        (
            TestStatus::Passed,
            format!(
                "No changed or causal review evidence returned for failed verdict: {}",
                response_description
            ),
        )
    } else if response_status == TestStatus::Failed
        && accepted_evidence.is_empty()
        && !is_infrastructure_failure(&response_description)
        && !is_absence_policy(&agent.instruction)
    {
        (
            TestStatus::Passed,
            format!(
                "No concrete review-scope evidence returned for failed verdict: {}",
                response_description
            ),
        )
    } else {
        (response_status, response_description)
    };
    AgentVerdict {
        test_id: agent.id.clone(),
        status,
        severity: response_severity.or(agent.severity),
        description,
        evidence: accepted_evidence,
        elapsed_ms,
    }
}

fn is_infrastructure_failure(description: &str) -> bool {
    description.contains("reached the step limit without returning a final verdict")
}

fn is_absence_policy(instruction: &str) -> bool {
    let lower = instruction.to_ascii_lowercase();
    lower.contains("doesn't contain")
        || lower.contains("does not contain")
        || lower.contains("missing")
        || lower.contains("absence")
        || lower.contains("no files")
}

pub(super) fn classify_evidence(
    evidence: &[Evidence],
    evidence_index: &HashSet<(String, u32)>,
    review_paths: &HashSet<String>,
    changed_lines: &HashSet<(String, u32)>,
    relevant_changed_lines: &HashSet<(String, u32)>,
) -> Vec<EvidenceClassificationReport> {
    evidence
        .iter()
        .map(|evidence| {
            let classification = classify_single_evidence(
                evidence,
                evidence_index,
                review_paths,
                changed_lines,
                relevant_changed_lines,
            )
            .unwrap_or(EvidenceClassification::OutsideReview);
            EvidenceClassificationReport {
                path: evidence.path.clone(),
                line: evidence.line,
                accepted: matches!(
                    classification,
                    EvidenceClassification::Changed | EvidenceClassification::ReviewContext
                ),
                classification,
            }
        })
        .collect()
}

fn classify_single_evidence(
    evidence: &Evidence,
    evidence_index: &HashSet<(String, u32)>,
    review_paths: &HashSet<String>,
    changed_lines: &HashSet<(String, u32)>,
    relevant_changed_lines: &HashSet<(String, u32)>,
) -> Option<EvidenceClassification> {
    let key = (evidence.path.clone(), evidence.line);
    if relevant_changed_lines.contains(&key) {
        return Some(EvidenceClassification::Changed);
    }
    if evidence_index.contains(&key)
        && (review_paths.is_empty() || review_paths.contains(&evidence.path))
    {
        return Some(EvidenceClassification::ReviewContext);
    }
    if changed_lines.contains(&key) {
        return Some(EvidenceClassification::UnfocusedChanged);
    }
    Some(EvidenceClassification::OutsideReview)
}

fn response_references_changed_context(response: &LlmResponse, terms: &HashSet<String>) -> bool {
    let mut haystack = response.description.to_ascii_lowercase();
    for evidence in &response.evidence {
        haystack.push('\n');
        haystack.push_str(&evidence.preview.to_ascii_lowercase());
    }
    terms
        .iter()
        .filter(|term| term.trim().chars().count() >= 4)
        .any(|term| haystack.contains(&term.to_ascii_lowercase()))
}
