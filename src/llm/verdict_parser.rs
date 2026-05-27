use super::bus::LlmBusError;
use super::types::LlmResponse;
use super::types::TestStatus;
use serde_json::Value;

pub fn parse_verdict(content: &str) -> Result<LlmResponse, LlmBusError> {
    parse_verdict_with_default_status(content, None)
}

pub fn parse_verdict_with_default_status(
    content: &str,
    default_status: Option<TestStatus>,
) -> Result<LlmResponse, LlmBusError> {
    let json = extract_json_object(content).unwrap_or(content).trim();
    let value: Value =
        serde_json::from_str(json).map_err(|_| LlmBusError::InvalidVerdict(content.to_string()))?;
    let value = normalize_verdict_value(value);
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status)
        .transpose()?
        .or(default_status)
        .ok_or_else(|| LlmBusError::InvalidVerdict(content.to_string()))?;
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let severity = match value.get("severity") {
        Some(Value::Null) | None => None,
        Some(Value::String(value)) if value == "null" => None,
        Some(severity) => serde_json::from_value(severity.clone())
            .map_err(|_| LlmBusError::InvalidVerdict(content.to_string()))?,
    };
    let evidence = match value.get("evidence") {
        Some(Value::Null) | None => Vec::new(),
        Some(evidence) => serde_json::from_value(evidence.clone())
            .map_err(|_| LlmBusError::InvalidVerdict(content.to_string()))?,
    };
    let description = description.unwrap_or_else(|| match status {
        TestStatus::Passed => "Provider returned passed without a description.".to_string(),
        TestStatus::Failed => "Provider returned failed without a description.".to_string(),
    });
    Ok(LlmResponse {
        status,
        severity,
        description,
        evidence,
    })
}

fn normalize_verdict_value(mut value: Value) -> Value {
    if let Some(evidence) = value.get_mut("evidence").and_then(Value::as_array_mut) {
        for item in evidence {
            let Some(object) = item.as_object_mut() else {
                continue;
            };
            if object.contains_key("preview") {
                continue;
            }
            for alias in ["line_preview", "linePreview", "code_preview", "codePreview"] {
                if let Some(preview) = object.get(alias).cloned() {
                    object.insert("preview".to_string(), preview);
                    break;
                }
            }
        }
    }
    value
}

fn parse_status(status: &str) -> Result<TestStatus, LlmBusError> {
    match status {
        "passed" => Ok(TestStatus::Passed),
        "failed" => Ok(TestStatus::Failed),
        _ => Err(LlmBusError::InvalidVerdict(status.to_string())),
    }
}

fn extract_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in content[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    let end = start + offset + ch.len_utf8();
                    return Some(&content[start..end]);
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;

    #[test]
    fn parses_provider_verdict_json() {
        let response = parse_verdict(
            r#"
            ```json
            {
              "status": "failed",
              "severity": "high",
              "description": "Missing retry around payment call.",
              "evidence": [
                {
                  "path": "src/payments.rs",
                  "line": 42,
                  "preview": "client.charge(request).await?"
                }
              ]
            }
            ```
            "#,
        )
        .unwrap();
        assert_eq!(response.status, TestStatus::Failed);
        assert_eq!(response.severity, Some(Severity::High));
        assert_eq!(response.evidence.len(), 1);
        assert_eq!(response.evidence[0].path, "src/payments.rs");
    }

    #[test]
    fn parses_provider_verdict_with_duplicate_status_key() {
        let response = parse_verdict(
            r#"{"status":"passed","severity":null,"description":"ok","evidence":[],"status":"passed"}"#,
        )
        .unwrap();
        assert_eq!(response.status, TestStatus::Passed);
        assert_eq!(response.description, "ok");
    }

    #[test]
    fn parses_provider_verdict_with_string_null_severity() {
        let response = parse_verdict(
            r#"{"status":"passed","severity":"null","description":"ok","evidence":[]}"#,
        )
        .unwrap();
        assert_eq!(response.status, TestStatus::Passed);
        assert_eq!(response.severity, None);
    }

    #[test]
    fn parses_provider_verdict_with_status_only() {
        let response = parse_verdict(r#"{"status":"passed"}"#).unwrap();
        assert_eq!(response.status, TestStatus::Passed);
        assert_eq!(
            response.description,
            "Provider returned passed without a description."
        );
        assert!(response.evidence.is_empty());
    }

    #[test]
    fn parses_provider_verdict_with_default_status() {
        let response = parse_verdict_with_default_status(
            r#"{"description":"safe route found","severity":"low","evidence":[{"path":"src/workflows.rs","line":189,"preview":"ensure_workflow_route();"}]}"#,
            Some(TestStatus::Passed),
        )
        .unwrap();
        assert_eq!(response.status, TestStatus::Passed);
        assert_eq!(response.severity, Some(Severity::Low));
        assert_eq!(response.evidence.len(), 1);
    }

    #[test]
    fn parses_provider_verdict_with_line_preview_alias() {
        let response = parse_verdict(
            r#"{"status":"failed","severity":"high","description":"bad","evidence":[{"path":"src/lib.rs","line":7,"line_preview":"bad();"}]}"#,
        )
        .unwrap();

        assert_eq!(response.status, TestStatus::Failed);
        assert_eq!(response.evidence[0].preview, "bad();");
    }

    #[test]
    fn parses_first_balanced_provider_verdict_before_trailing_junk() {
        let response = parse_verdict(
            r#"{"status":"failed","severity":"high","description":"bad","evidence":[{"path":"src/lib.rs","line":7,"preview":"if value == \"}\" { bad(); }"}]}]}]"#,
        )
        .unwrap();

        assert_eq!(response.status, TestStatus::Failed);
        assert_eq!(response.evidence[0].preview, "if value == \"}\" { bad(); }");
    }
}
