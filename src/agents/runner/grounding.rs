use super::*;

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
    let hunk_packet_tokens = estimate_tokens(&hunk_packet);
    let full_packet_tokens = estimate_tokens(&format!("{file_inventory}\n\n{hunk_packet}"));
    let context = if !hunks.is_empty() && full_packet_tokens <= agent.initial_context_token_budget {
        format!(
            "{file_inventory}\n\n{hunk_packet}\n\nChanged lines above are the primary review evidence. You may return a final verdict immediately if the changed-line packet is sufficient. If surrounding code, helper behavior, definitions, references, or callers are needed, request tools and continue the loop. Prefer get_hunk_context with a hunk id for targeted surrounding code before whole-file reads. Final failed evidence should point to changed lines or review-scope context directly caused by the change."
        )
    } else if !hunks.is_empty() {
        let hunk_summary = format_review_hunk_summary(&hunks);
        format!(
            "{file_inventory}\n\nReview-scope changed-line packet is too large to include directly ({} hunks, about {} tokens). Hunk summary:\n{}\n\nUse get_hunk_context with a hunk id or list_review_hunks for targeted details before returning a verdict. Use read_file only when the hunk context is insufficient.",
            hunks.len(),
            hunk_packet_tokens,
            hunk_summary
        )
    } else {
        format!(
            "{file_inventory}\nOnly fail when the concrete issue is in one of these review-scope files. You may use list_files, search_text, read_file, get_file_context, find_definitions, or find_references to gather context from the wider repository when needed, but final failed evidence must come from review-scope files."
        )
    };
    let evidence_index = HashSet::new();
    let changed_lines = changed_lines_for_hunks(&hunks);
    let review_causal_terms = review_causal_terms_for_hunks(&hunks);

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
        review_causal_terms,
    })
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

fn review_causal_terms_for_hunks(hunks: &[ReviewHunk]) -> HashSet<String> {
    let mut terms = HashSet::new();
    for hunk in hunks {
        terms.insert(hunk.id.clone());
        for line in &hunk.lines {
            let trimmed = line.content.trim();
            if trimmed.chars().count() >= 4 {
                terms.insert(trimmed.to_string());
            }
            for token in symbol_tokens(trimmed) {
                terms.insert(token);
            }
        }
    }
    terms
}

fn symbol_tokens(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
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
    }
    summary
}
