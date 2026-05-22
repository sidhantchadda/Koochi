use super::bus::LlmBusError;
use super::types::Evidence;
use super::types::LlmResponse;
use super::types::TestStatus;
use crate::Severity;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct VerdictJson {
    status: TestStatusJson,
    severity: Option<Severity>,
    description: String,
    #[serde(default)]
    evidence: Vec<Evidence>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TestStatusJson {
    Passed,
    Failed,
}

pub fn parse_verdict(content: &str) -> Result<LlmResponse, LlmBusError> {
    let json = extract_json_object(content).unwrap_or(content).trim();
    let verdict: VerdictJson =
        serde_json::from_str(json).map_err(|_| LlmBusError::InvalidVerdict(content.to_string()))?;
    Ok(LlmResponse {
        status: match verdict.status {
            TestStatusJson::Passed => TestStatus::Passed,
            TestStatusJson::Failed => TestStatus::Failed,
        },
        severity: verdict.severity,
        description: verdict.description,
        evidence: verdict.evidence,
    })
}

fn extract_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    (start <= end).then_some(&content[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
