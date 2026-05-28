use super::*;
use crate::search::TextMatch;

pub(super) async fn build_grounded_request<S>(
    agent: &AgentSpec,
    search: &S,
) -> Result<GroundedRequest, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    let files = search
        .list_review_files(ListFilesRequest {
            kind: FileKind::Source,
        })
        .await
        .map_err(|err| AgentError::Search(err.to_string()))?
        .files;
    let hunks = search
        .list_review_hunks()
        .await
        .map_err(|err| AgentError::Search(err.to_string()))?
        .hunks;
    let full_repo_mode = matches!(search.review_mode(), Some(ReviewMode::FullRepo));
    let full_repo_context_mode = matches!(
        search.review_mode(),
        Some(ReviewMode::FullRepo | ReviewMode::FullRepoFallback)
    );

    let file_count = files.len();
    let review_paths = files.iter().cloned().collect::<HashSet<_>>();
    let shown_files = files
        .iter()
        .take(MAX_CONTEXT_FILES)
        .map(|path| format!("- {path}"))
        .collect::<Vec<_>>()
        .join("\n");
    let file_inventory = if file_count > MAX_CONTEXT_FILES {
        format!(
            "Review-scope source file inventory ({file_count} total, first {MAX_CONTEXT_FILES} shown):\n{shown_files}"
        )
    } else {
        format!("Review-scope source file inventory ({file_count} total):\n{shown_files}")
    };
    let hunk_packet = format_review_hunk_packet(&hunks);
    let focus = InvariantFocus::new(&agent.id, &agent.instruction, &hunks);
    let target_context_line =
        instruction_target_context_line(&agent.instruction, &hunks, &review_paths, search).await?;
    let focus_summary = focus.format_summary(target_context_line.as_ref());
    let hunk_packet_tokens = estimate_tokens(&hunk_packet);
    let full_packet_tokens = estimate_tokens(&format!(
        "{file_inventory}\n\n{focus_summary}\n\n{hunk_packet}"
    ));
    let allows_direct_verdict =
        !hunks.is_empty() && full_packet_tokens <= agent.initial_context_token_budget;
    let context = if allows_direct_verdict {
        format!(
            "{file_inventory}\n\n{focus_summary}\n\n{hunk_packet}\n\nChanged lines above are the primary review evidence. Return `passed` only when this context is sufficient to show the invariant is satisfied. Do not return `failed` from this packet alone. Failed verdicts require targeted content inspection first with get_hunk_context, get_file_context, or read_file. Prefer get_hunk_context with a hunk id for targeted surrounding code before whole-file reads. Final failed evidence should point to focused changed lines or review-scope context directly caused by the change."
        )
    } else if !hunks.is_empty() {
        let hunk_summary = format_review_hunk_summary(&hunks);
        format!(
            "{file_inventory}\n\n{focus_summary}\n\nReview-scope changed-line packet is too large to include directly ({} hunks, about {} tokens). Hunk summary:\n{}\n\nUse get_hunk_context with a hunk id or list_review_hunks for targeted details before returning a verdict. Use read_file only when the hunk context is insufficient.",
            hunks.len(),
            hunk_packet_tokens,
            hunk_summary
        )
    } else if full_repo_context_mode {
        let search_terms = full_repo_search_terms(&focus.terms);
        let search_terms_hint = if search_terms.is_empty() {
            "(none)".to_string()
        } else {
            search_terms.join(", ")
        };
        format!(
            "{file_inventory}\n\n{focus_summary}\n\nFull-repo mode is active. There is no changed diff or hunk packet. The review-scope source files above are the repository code under review for this run. Koochi will not accept a passed verdict until it has shown this agent every review-scope source chunk. If an invariant says \"changed code\", \"changed <area>\", or similar diff-oriented wording, interpret that as \"review-scope repository code\" in full-repo mode; do not return passed merely because no diff exists. Suggested full-repo search terms from this invariant: {search_terms_hint}. Use search_text, get_file_context, read_file, find_definitions, or find_references for targeted investigation when useful. Do not call list_files first; Koochi already supplied a source-file inventory preview, and broad file listing is not investigation. Failed verdicts require targeted content inspection first with get_file_context or read_file, and final failed evidence may come from any inspected review-scope source file or delivered coverage chunk that demonstrates the invariant violation."
        )
    } else {
        format!(
            "{file_inventory}\n\n{focus_summary}\n\nKoochi will not accept a passed verdict until it has shown this agent every review-scope source chunk. In commit, range, and local-change modes, review-scope source files are the changed source files for that review target. Only fail when the concrete issue is in one of these review-scope files. You may use list_files, search_text, read_file, get_file_context, find_definitions, or find_references to gather context from the wider repository when needed, but final failed evidence must come from review-scope files or delivered coverage chunks."
        )
    };
    let evidence_index = HashSet::new();
    let changed_lines = changed_lines_for_hunks(&hunks);
    let target_context_line = target_context_line.clone();
    let focused_context_line = focus.first_relevant_changed_line.clone();
    let relevant_changed_lines = focus.relevant_changed_lines;
    let review_causal_terms = focus.review_causal_terms;
    let full_repo_search_terms = full_repo_search_terms(&focus.terms);

    let instruction = grounded_agent_prompt(&agent.instruction, context.trim());

    Ok(GroundedRequest {
        request: LlmRequest {
            test_id: agent.id.clone(),
            model: agent.model.clone(),
            instruction,
        },
        evidence_index,
        review_paths,
        changed_lines,
        target_context_line,
        focused_context_line,
        relevant_changed_lines,
        review_causal_terms,
        allows_direct_verdict,
        full_repo_mode,
        full_repo_search_terms,
    })
}

fn full_repo_search_terms(terms: &[String]) -> Vec<String> {
    let mut terms = terms
        .iter()
        .filter(|term| term.chars().count() >= 4)
        .filter(|term| !matches!(term.as_str(), "validation" | "integrity" | "preserved"))
        .cloned()
        .collect::<Vec<_>>();
    terms.sort_by_key(|term| {
        (
            generic_full_repo_search_term(term),
            std::cmp::Reverse(term.chars().count()),
        )
    });
    terms.truncate(FULL_REPO_REQUIRED_SEARCH_TERMS);
    terms
}

fn generic_full_repo_search_term(term: &str) -> bool {
    matches!(
        term,
        "validation"
            | "handling"
            | "integrity"
            | "preserved"
            | "protection"
            | "boundary"
            | "boundaries"
            | "runtime"
    )
}

fn changed_lines_for_hunks(hunks: &[ReviewHunk]) -> HashSet<(String, u32)> {
    hunks
        .iter()
        .flat_map(|hunk| {
            hunk.lines.iter().filter_map(|line| {
                let line_number = match line.kind {
                    ReviewLineKind::Added | ReviewLineKind::Context => line.new_line,
                    ReviewLineKind::Removed => line.old_line,
                }?;
                Some((hunk.path.clone(), line_number))
            })
        })
        .collect()
}

#[derive(Debug)]
struct InvariantFocus {
    terms: Vec<String>,
    matched_lines: Vec<String>,
    first_relevant_changed_line: Option<(String, u32)>,
    relevant_changed_lines: HashSet<(String, u32)>,
    review_causal_terms: HashSet<String>,
}

impl InvariantFocus {
    fn new(id: &str, instruction: &str, hunks: &[ReviewHunk]) -> Self {
        let terms = invariant_focus_terms(id, instruction);
        let term_set = terms.iter().cloned().collect::<HashSet<_>>();
        let mut matched_lines = Vec::new();
        let mut first_relevant_changed_line = None;
        let mut relevant_changed_lines = HashSet::new();
        let mut review_causal_terms = HashSet::new();
        for hunk in hunks {
            for line in &hunk.lines {
                let Some(line_number) = changed_line_number(line) else {
                    continue;
                };
                if !substantive_changed_line(&line.content) {
                    continue;
                }
                let line_terms = symbol_tokens(&line.content)
                    .into_iter()
                    .map(|term| term.to_ascii_lowercase())
                    .collect::<HashSet<_>>();
                let mut matched = line_terms
                    .iter()
                    .filter(|term| term_set.contains(*term))
                    .cloned()
                    .collect::<Vec<_>>();
                matched.sort();
                if matched.is_empty() {
                    continue;
                }
                if first_relevant_changed_line.is_none() {
                    first_relevant_changed_line = Some((hunk.path.clone(), line_number));
                }
                relevant_changed_lines.insert((hunk.path.clone(), line_number));
                review_causal_terms.extend(matched.iter().cloned());
                if matched_lines.len() < 16 {
                    matched_lines.push(format!(
                        "- hunk_id={} {}:{} [{}] {}",
                        hunk.id,
                        hunk.path,
                        line_number,
                        matched.join(", "),
                        line.content.trim()
                    ));
                }
            }
        }
        Self {
            terms,
            matched_lines,
            first_relevant_changed_line,
            relevant_changed_lines,
            review_causal_terms,
        }
    }

    fn format_summary(&self, target_context_line: Option<&(String, u32)>) -> String {
        let terms = if self.terms.is_empty() {
            "(none)".to_string()
        } else {
            self.terms.join(", ")
        };
        let target_hint = target_context_line
            .map(|(path, line)| {
                format!("\nExact target symbol line for get_file_context: {path}:{line}")
            })
            .unwrap_or_default();
        if self.matched_lines.is_empty() {
            format!(
                "Invariant focus terms: {terms}{target_hint}\nNo substantive changed line directly matches these focus terms. Failed verdicts require targeted content inspection with get_hunk_context, get_file_context, or read_file before they can be accepted."
            )
        } else {
            format!(
                "Invariant focus terms: {terms}{target_hint}\nSubstantive changed lines matching invariant focus, with hunk ids for get_hunk_context:\n{}",
                self.matched_lines.join("\n")
            )
        }
    }
}

async fn instruction_target_context_line<S>(
    instruction: &str,
    hunks: &[ReviewHunk],
    review_paths: &HashSet<String>,
    search: &S,
) -> Result<Option<(String, u32)>, AgentError>
where
    S: CodeSearchApi + ?Sized,
    S::Error: Display,
{
    if let Some(target) = instruction_hunk_target_context_line(instruction, hunks) {
        return Ok(Some(target));
    }

    let Some(target_symbol) = instruction_target_symbol(instruction) else {
        return Ok(None);
    };
    let target_path = instruction_target_path(instruction);
    let response = search
        .search_text(SearchTextRequest {
            query: target_symbol.clone(),
            kind: FileKind::Source,
        })
        .await
        .map_err(|err| AgentError::Search(err.to_string()))?;
    Ok(best_target_symbol_match(
        &target_symbol,
        target_path.as_deref(),
        &review_paths,
        response.matches,
    ))
}

fn instruction_hunk_target_context_line(
    instruction: &str,
    hunks: &[ReviewHunk],
) -> Option<(String, u32)> {
    let backticked = backticked_terms(instruction);
    let target_symbol = instruction_target_symbol_from_terms(&backticked)?;
    let target_path = instruction_target_path_from_terms(&backticked);
    for hunk in hunks {
        if let Some(path) = target_path
            && hunk.path != *path
        {
            continue;
        }
        for line in &hunk.lines {
            if line.content.contains(target_symbol)
                && let Some(line_number) = changed_line_number(line)
            {
                return Some((hunk.path.clone(), line_number));
            }
        }
    }
    None
}

fn instruction_target_symbol(instruction: &str) -> Option<String> {
    instruction_target_symbol_from_terms(&backticked_terms(instruction)).cloned()
}

fn instruction_target_symbol_from_terms(terms: &[String]) -> Option<&String> {
    terms
        .iter()
        .find(|term| is_target_symbol_term(term) && term.chars().any(|ch| ch == '_'))
        .or_else(|| terms.iter().find(|term| is_target_symbol_term(term)))
}

fn instruction_target_path(instruction: &str) -> Option<String> {
    instruction_target_path_from_terms(&backticked_terms(instruction)).cloned()
}

fn instruction_target_path_from_terms(terms: &[String]) -> Option<&String> {
    terms
        .iter()
        .find(|term| term.contains('/') || looks_like_source_path(term))
}

fn is_target_symbol_term(term: &str) -> bool {
    !term.is_empty()
        && !term.contains('/')
        && !term.contains('.')
        && term
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && term.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn looks_like_source_path(term: &str) -> bool {
    [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".kt", ".swift", ".c", ".cc",
        ".cpp", ".h", ".hpp",
    ]
    .iter()
    .any(|suffix| term.ends_with(suffix))
}

fn best_target_symbol_match(
    target_symbol: &str,
    target_path: Option<&str>,
    review_paths: &HashSet<String>,
    matches: Vec<TextMatch>,
) -> Option<(String, u32)> {
    let mut candidates = matches
        .into_iter()
        .filter(|text_match| review_paths.contains(&text_match.path))
        .filter(|text_match| target_path.is_none_or(|target_path| text_match.path == target_path))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|text_match| {
        (
            target_symbol_match_rank(target_symbol, &text_match.preview),
            text_match.path.clone(),
            text_match.line,
        )
    });
    candidates
        .into_iter()
        .next()
        .map(|text_match| (text_match.path, text_match.line))
}

fn target_symbol_match_rank(target_symbol: &str, preview: &str) -> u8 {
    let lower = preview.to_ascii_lowercase();
    let target = target_symbol.to_ascii_lowercase();
    let definition_needles = [
        format!("fn {target}"),
        format!("function {target}"),
        format!("const {target}"),
        format!("let {target}"),
        format!("def {target}"),
        format!("class {target}"),
        format!("struct {target}"),
    ];
    if definition_needles
        .iter()
        .any(|needle| lower.contains(needle))
    {
        0
    } else {
        1
    }
}

fn backticked_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut in_backticks = false;
    for part in value.split('`') {
        if in_backticks {
            terms.push(part.trim().to_string());
        }
        in_backticks = !in_backticks;
    }
    terms
}

fn changed_line_number(line: &crate::scope::ReviewHunkLine) -> Option<u32> {
    match line.kind {
        ReviewLineKind::Added | ReviewLineKind::Context => line.new_line,
        ReviewLineKind::Removed => line.old_line,
    }
}

fn invariant_focus_terms(id: &str, instruction: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for source in [id, instruction] {
        for token in symbol_tokens(source) {
            let token = token.to_ascii_lowercase();
            if is_focus_stopword(&token) || !seen.insert(token.clone()) {
                continue;
            }
            terms.push(token);
        }
    }
    terms
}

fn is_focus_stopword(token: &str) -> bool {
    matches!(
        token,
        "able"
            | "about"
            | "active"
            | "against"
            | "allows"
            | "before"
            | "being"
            | "browser"
            | "changed"
            | "changes"
            | "check"
            | "claim"
            | "claims"
            | "code"
            | "configured"
            | "concrete"
            | "content"
            | "could"
            | "data"
            | "does"
            | "doing"
            | "explicit"
            | "evidence"
            | "fail"
            | "file"
            | "files"
            | "find"
            | "from"
            | "function"
            | "handler"
            | "handling"
            | "helper"
            | "helpers"
            | "including"
            | "immediate"
            | "intended"
            | "logic"
            | "must"
            | "other"
            | "path"
            | "paths"
            | "request"
            | "requests"
            | "response"
            | "returns"
            | "review"
            | "route"
            | "scope"
            | "serve"
            | "server"
            | "should"
            | "safe"
            | "static"
            | "that"
            | "this"
            | "type"
            | "unless"
            | "uses"
            | "when"
            | "where"
            | "with"
            | "without"
    )
}

pub(super) fn substantive_changed_line(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().all(|ch| "{}[]();,.".contains(ch)) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("import ")
        || lower.starts_with("export type ")
        || lower.starts_with("type ")
        || lower.starts_with("//")
        || lower.starts_with("*")
        || lower == "}"
        || lower == "};"
    {
        return false;
    }
    true
}

fn symbol_tokens(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.chars().count() >= 4)
        .map(ToString::to_string)
        .collect()
}

fn format_review_hunk_packet(hunks: &[ReviewHunk]) -> String {
    let mut packet = format!("Review-scope changed hunks ({} total):", hunks.len());
    for hunk in hunks {
        packet.push_str(&format!(
            "\n\n--- hunk {} {} -{},{} +{},{}",
            hunk.id, hunk.path, hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        ));
        for line in &hunk.lines {
            let (prefix, line_no) = match line.kind {
                ReviewLineKind::Added => ("+", line.new_line),
                ReviewLineKind::Removed => ("-", line.old_line),
                ReviewLineKind::Context => (" ", line.new_line.or(line.old_line)),
            };
            let line_no = line_no
                .map(|line| line.to_string())
                .unwrap_or_else(|| "-".to_string());
            packet.push_str(&format!("\n{prefix}{line_no}: {}", line.content));
        }
    }
    packet
}

fn format_review_hunk_summary(hunks: &[ReviewHunk]) -> String {
    let file_count = hunks
        .iter()
        .map(|hunk| hunk.path.as_str())
        .collect::<HashSet<_>>()
        .len();
    let mut summary = format!("{} changed files, {} hunks", file_count, hunks.len());
    for hunk in hunks {
        summary.push_str(&format!(
            "\n- {} {} -{},{} +{},{} ({} lines)",
            hunk.id,
            hunk.path,
            hunk.old_start,
            hunk.old_lines,
            hunk.new_start,
            hunk.new_lines,
            hunk.lines.len()
        ));
        let previews = hunk
            .lines
            .iter()
            .filter_map(format_hunk_preview_line)
            .take(4)
            .collect::<Vec<_>>();
        if !previews.is_empty() {
            summary.push_str("\n    focused preview:");
            for preview in previews {
                summary.push_str(&format!("\n    {preview}"));
            }
        }
    }
    summary
}

fn format_hunk_preview_line(line: &crate::scope::ReviewHunkLine) -> Option<String> {
    if !substantive_changed_line(&line.content) {
        return None;
    }
    let line_number = changed_line_number(line)?;
    let prefix = match line.kind {
        ReviewLineKind::Added => "+",
        ReviewLineKind::Removed => "-",
        ReviewLineKind::Context => " ",
    };
    Some(format!("{prefix}{line_number}: {}", line.content.trim()))
}
