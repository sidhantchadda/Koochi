use super::*;

pub(crate) fn print_agent_progress(event: &AgentProgressEvent) {
    match event {
        AgentProgressEvent::BatchPreparing {
            batch_index,
            batch_count,
            agent_count,
        } => println!("Koochi: preparing batch {batch_index}/{batch_count} ({agent_count} agents)"),
        AgentProgressEvent::BatchCallingLlm {
            batch_index,
            batch_count,
            agent_count,
        } => println!(
            "Koochi: running LLM loop for batch {batch_index}/{batch_count} ({agent_count} agents)"
        ),
        AgentProgressEvent::AgentCompleted {
            test_id,
            completed_agents,
            total_agents,
            running_agent_ids,
        } => println!(
            "{completed_agents}/{total_agents} invariant agents completed. Last finished: {test_id}. Still running: {}",
            running_agent_ids.join(", ")
        ),
        AgentProgressEvent::ProgressTick { .. } => {}
        AgentProgressEvent::BatchCompleted {
            batch_index,
            batch_count,
            agent_count,
            llm_calls,
            llm_elapsed,
            ..
        } => println!(
            "Koochi: completed batch {batch_index}/{batch_count} ({agent_count} agents, {llm_calls} LLM calls, LLM {})",
            format_duration(*llm_elapsed)
        ),
    }
}

pub(crate) fn print_live_agent_progress(event: &AgentProgressEvent, verbose: bool) {
    let (completed_agents, total_agents, running_agent_ids) = match event {
        AgentProgressEvent::AgentCompleted {
            completed_agents,
            total_agents,
            running_agent_ids,
            ..
        }
        | AgentProgressEvent::ProgressTick {
            completed_agents,
            total_agents,
            running_agent_ids,
        } => (*completed_agents, *total_agents, running_agent_ids),
        _ => return,
    };
    let spinner = live_spinner();
    let mut line =
        format!("{spinner} {completed_agents}/{total_agents} invariant agents completed.");
    if verbose && !running_agent_ids.is_empty() {
        let running = running_agent_ids
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let remaining = running_agent_ids.len().saturating_sub(8);
        if remaining > 0 {
            line.push_str(&format!(" Still running: {running}, +{remaining} more"));
        } else {
            line.push_str(&format!(" Still running: {running}"));
        }
    }
    print!("\r\x1b[2K{line}");
    let _ = std::io::stdout().flush();
}

fn live_spinner() -> &'static str {
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() / 150)
        .unwrap_or_default();
    ["|", "/", "-", "\\"][(tick as usize) % 4]
}

pub(crate) fn clear_live_agent_progress() {
    print!("\r\x1b[2K");
    let _ = std::io::stdout().flush();
}

pub(crate) fn print_trace_event(event: AgentTraceEvent, verbose: bool) {
    match event {
        AgentTraceEvent::Started { test_id, max_steps } => {
            println!("trace: started {test_id} (max {max_steps} steps)");
        }
        AgentTraceEvent::StepStarted {
            step,
            prompt_tokens,
            prompt,
        } => {
            println!();
            println!("trace: step {step} ({prompt_tokens} tokens)");
            if verbose {
                println!("  {}", cyan("input prompt:"));
                println!(
                    "{}",
                    dim(&indent_for_trace(&middle_truncate_for_trace(
                        &prompt, 12_000
                    )))
                );
            }
        }
        AgentTraceEvent::LlmAction {
            step: _,
            action,
            output,
        } => {
            println!("  llm: {action}");
            if verbose {
                println!("  {}", green("model output:"));
                println!(
                    "{}",
                    indent_for_trace(&middle_truncate_for_trace(&output, 6_000))
                );
            }
        }
        AgentTraceEvent::InvalidResponse { step: _, content } => {
            println!("  {}", yellow("rejected: invalid provider response"));
            println!("    {}", compact_for_trace(&content, 1200));
        }
        AgentTraceEvent::PrematureFinal { step: _, guidance } => {
            println!("  {}", yellow("rejected: premature final verdict"));
            println!("    {}", compact_for_trace(&guidance, 1200));
        }
        AgentTraceEvent::EvidenceClassified { items } => {
            if verbose && !items.is_empty() {
                println!("  {}", dim("evidence classification:"));
                for item in items {
                    let label = match item.classification {
                        crate::agents::EvidenceClassification::Changed => green("changed-line"),
                        crate::agents::EvidenceClassification::ReviewContext => {
                            yellow("review-context")
                        }
                        crate::agents::EvidenceClassification::OutsideReview => {
                            red("outside-review")
                        }
                    };
                    let verdict = if item.accepted {
                        "accepted"
                    } else {
                        "rejected"
                    };
                    println!("    - {}:{} {label} {verdict}", item.path, item.line);
                }
            }
        }
        AgentTraceEvent::ToolExecuted {
            step: _,
            tool,
            observation,
        } => {
            println!("  tool: {tool}");
            println!("  observation: {}", summarize_observation(&observation));
        }
        AgentTraceEvent::FinalVerdict { step: _, response } => {
            println!(
                "  final: {:?} severity={:?} evidence={}",
                response.status,
                response.severity,
                response.evidence.len()
            );
            println!("    {}", response.description);
        }
        AgentTraceEvent::StepLimit { response } => {
            println!("  step limit: {:?}", response.status);
            println!("    {}", response.description);
        }
    }
}

fn summarize_observation(observation: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(observation) else {
        return compact_for_trace(observation, 1200);
    };
    if let Some(files) = value.get("files").and_then(|value| value.as_array()) {
        return format!("{} files: {}", files.len(), preview_json_items(files, 8));
    }
    if let Some(matches) = value.get("matches").and_then(|value| value.as_array()) {
        return format!(
            "{} matches: {}",
            matches.len(),
            preview_locations(matches, 8)
        );
    }
    if let Some(hunks) = value.get("hunks").and_then(|value| value.as_array()) {
        return format!("{} hunks: {}", hunks.len(), preview_hunks(hunks, 8));
    }
    if let Some(definitions) = value.get("definitions").and_then(|value| value.as_array()) {
        return format!(
            "{} definitions: {}",
            definitions.len(),
            preview_locations(definitions, 8)
        );
    }
    if let Some(references) = value.get("references").and_then(|value| value.as_array()) {
        return format!(
            "{} references: {}",
            references.len(),
            preview_locations(references, 8)
        );
    }
    if let Some(path) = value.get("path").and_then(|value| value.as_str()) {
        let line_count = value
            .get("line_count")
            .and_then(|value| value.as_u64())
            .map(|line_count| format!("{line_count} lines"))
            .or_else(|| {
                let start = value.get("start_line")?.as_u64()?;
                let end = value.get("end_line")?.as_u64()?;
                Some(format!("lines {start}-{end}"))
            })
            .unwrap_or_else(|| "file content".to_string());
        if let Some(hunk_id) = value.get("hunk_id").and_then(|value| value.as_str()) {
            return format!("{path} {hunk_id} ({line_count})");
        }
        return format!("{path} ({line_count})");
    }
    compact_for_trace(observation, 1200)
}

fn preview_hunks(items: &[serde_json::Value], limit: usize) -> String {
    let shown = items
        .iter()
        .take(limit)
        .map(|item| {
            let id = item
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let path = item
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            format!("{id} {path}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    let remaining = items.len().saturating_sub(limit);
    if remaining > 0 {
        format!("{shown}; +{remaining} more")
    } else {
        shown
    }
}

fn preview_locations(items: &[serde_json::Value], limit: usize) -> String {
    let shown = items
        .iter()
        .take(limit)
        .map(|item| {
            let path = item
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let line = item
                .get("line")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let preview = item
                .get("preview")
                .and_then(|value| value.as_str())
                .map(|preview| format!(" {}", compact_for_trace(preview, 90)))
                .unwrap_or_default();
            format!("{path}:{line}{preview}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    let remaining = items.len().saturating_sub(limit);
    if remaining > 0 {
        format!("{shown}; +{remaining} more")
    } else {
        shown
    }
}

fn preview_json_items(items: &[serde_json::Value], limit: usize) -> String {
    let shown = items
        .iter()
        .take(limit)
        .map(|item| {
            item.as_str()
                .map(ToString::to_string)
                .unwrap_or_else(|| item.to_string())
        })
        .collect::<Vec<_>>()
        .join(", ");
    let remaining = items.len().saturating_sub(limit);
    if remaining > 0 {
        format!("{shown}, +{remaining} more")
    } else {
        shown
    }
}

fn compact_for_trace(value: &str, max_chars: usize) -> String {
    let mut compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > max_chars {
        compact = compact.chars().take(max_chars).collect::<String>();
        compact.push_str("...");
    }
    compact
}

fn middle_truncate_for_trace(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_string();
    }
    let edge_chars = max_chars.saturating_sub(160) / 2;
    let start = value.chars().take(edge_chars).collect::<String>();
    let end = value
        .chars()
        .rev()
        .take(edge_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!(
        "{start}\n\n... trace prompt truncated: {} chars omitted ...\n\n{end}",
        char_count.saturating_sub(edge_chars * 2)
    )
}

fn indent_for_trace(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn review_scope_line(review: &crate::scope::ReviewScope) -> String {
    let changed_loc = review_changed_loc(review);
    let changed_loc = format_changed_loc(changed_loc);
    match review.mode {
        ReviewMode::HeadCommit => {
            if let Some(commit) = &review.commit {
                format!(
                    "Koochi: {} {} ({changed_loc})",
                    commit.short_id, commit.subject
                )
            } else {
                format!("Koochi: HEAD ({changed_loc})")
            }
        }
        ReviewMode::LocalChanges => format!("Koochi: local changes ({changed_loc})"),
        ReviewMode::FullRepoFallback => format!("Koochi: full repo fallback"),
    }
}

fn review_changed_loc(review: &crate::scope::ReviewScope) -> usize {
    review
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| {
            matches!(
                line.kind,
                crate::scope::ReviewLineKind::Added | crate::scope::ReviewLineKind::Removed
            )
        })
        .count()
}

fn format_changed_loc(changed_loc: usize) -> String {
    match changed_loc {
        1 => "1 LOC changed".to_string(),
        count => format!("{count} LOC changed"),
    }
}

pub(crate) fn print_report(
    report: &SynthesisReport,
    elapsed: Duration,
    token_usage: LlmTokenUsage,
) {
    let total = report.passed.len() + report.failed.len();
    println!();
    for verdict in &report.failed {
        let severity = severity_label(verdict.severity);
        println!(
            "- [{}] {}: {}",
            severity, verdict.test_id, verdict.description
        );
        if verdict.evidence.is_empty() {
            println!("  {} none returned", dim("evidence:"));
        } else {
            println!("  {}", dim("evidence:"));
            for evidence in &verdict.evidence {
                println!(
                    "    - {}:{} {}",
                    cyan(&evidence.path),
                    yellow(&evidence.line.to_string()),
                    dim(&evidence.preview)
                );
            }
        }
    }
    if !report.failed.is_empty() {
        println!();
    }

    let status = summary_status(report);
    let token_suffix = if token_usage.total_tokens > 0 {
        format!(", {} tokens used", format_count(token_usage.total_tokens))
    } else {
        String::new()
    };
    let summary = format!(
        "Finished in {}: {}/{} passed, {} failed{}",
        format_duration(elapsed),
        report.passed.len(),
        total,
        report.failed.len(),
        token_suffix
    );
    println!("{}", color_for_status(status, &summary));
}

pub(crate) fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    formatted.chars().rev().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryStatus {
    Pass,
    Warning,
    Error,
}

fn summary_status(report: &SynthesisReport) -> SummaryStatus {
    if report.failed.is_empty() {
        SummaryStatus::Pass
    } else if report
        .failed
        .iter()
        .any(|verdict| matches!(verdict.severity, Some(Severity::High | Severity::Critical)))
    {
        SummaryStatus::Error
    } else {
        SummaryStatus::Warning
    }
}

fn severity_label(severity: Option<Severity>) -> String {
    match severity {
        Some(Severity::Critical) => red("Critical"),
        Some(Severity::High) => red("High"),
        Some(Severity::Medium) => yellow("Medium"),
        Some(Severity::Low) => cyan("Low"),
        None => dim("Unknown"),
    }
}

fn color_for_status(status: SummaryStatus, text: &str) -> String {
    match status {
        SummaryStatus::Pass => green(text),
        SummaryStatus::Warning => yellow(text),
        SummaryStatus::Error => red(text),
    }
}

pub(crate) fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

pub(crate) fn green(text: &str) -> String {
    ansi("32", text)
}

pub(crate) fn yellow(text: &str) -> String {
    ansi("33", text)
}

pub(crate) fn red(text: &str) -> String {
    ansi("31", text)
}

pub(crate) fn cyan(text: &str) -> String {
    ansi("36", text)
}

pub(crate) fn dim(text: &str) -> String {
    ansi("2", text)
}

fn ansi(code: &str, text: &str) -> String {
    format!("\x1b[{code}m{text}\x1b[0m")
}
