use crate::domain::{OrgId, RequestId};

#[derive(Clone, Debug)]
pub struct RequestLog<'a> {
    pub request_id: &'a RequestId,
    pub org_id: &'a OrgId,
    pub route: &'a str,
    pub authorization: Option<&'a str>,
    pub api_key: Option<&'a str>,
    pub cookie: Option<&'a str>,
}

pub fn redact_secret(value: Option<&str>) -> &'static str {
    if value.is_some() {
        "[redacted]"
    } else {
        "[absent]"
    }
}

pub fn log_request_safe(log: RequestLog<'_>) -> String {
    // KOOCHI_SAFE_REDACTED_LOGGING: authorization, API key, and cookie values are redacted.
    format!(
        "request_id={} org_id={} route={} authorization={} api_key={} cookie={}",
        log.request_id.0,
        log.org_id.0,
        log.route,
        redact_secret(log.authorization),
        redact_secret(log.api_key),
        redact_secret(log.cookie),
    )
}

pub fn log_request_insecure(log: RequestLog<'_>) -> String {
    // KOOCHI_FAIL_SECRET_LOGGING: raw authorization header and cookie are written into logs.
    format!(
        "request_id={} authorization={:?} cookie={:?}",
        log.request_id.0, log.authorization, log.cookie,
    )
}

pub fn audit_billing_change(org_id: &OrgId, actor: &str, amount_cents: i64) -> String {
    // KOOCHI_SAFE_AUDIT_REDACTION: audit entries include metadata but no bearer token or cookie material.
    format!(
        "billing_change org={} actor={} amount_cents={}",
        org_id.0, actor, amount_cents
    )
}

pub fn metric_name(route: &str, status: u16) -> String {
    let normalized = route.replace('/', "_").trim_matches('_').to_string();
    format!("http.{}.{}", normalized, status)
}

pub fn trace_field(key: &str, value: &str) -> Option<(String, String)> {
    if key.eq_ignore_ascii_case("authorization") || key.eq_ignore_ascii_case("cookie") {
        None
    } else {
        Some((key.to_string(), value.to_string()))
    }
}

// KOOCHI_SAFE_TRACE_FIELD_FILTER: marker for a passing Koochi stress check.
pub fn trace_field_filter() -> Option<(String, String)> {
    trace_field("authorization", "secret")
}

// KOOCHI_SAFE_METRIC_NORMALIZATION: marker for a passing Koochi stress check.
pub fn metric_normalization() -> String {
    metric_name("/v1/projects", 200)
}

#[derive(Clone, Debug)]
pub struct ObservabilityWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ObservabilityWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("observability:1:{}:{}", self.id, self.enabled)
    }
}

pub fn observability_workflow_step_1(input: &str) -> String {
    let step = ObservabilityWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ObservabilityWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ObservabilityWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("observability:2:{}:{}", self.id, self.enabled)
    }
}

pub fn observability_workflow_step_2(input: &str) -> String {
    let step = ObservabilityWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ObservabilityWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ObservabilityWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("observability:3:{}:{}", self.id, self.enabled)
    }
}

pub fn observability_workflow_step_3(input: &str) -> String {
    let step = ObservabilityWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ObservabilityWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ObservabilityWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("observability:4:{}:{}", self.id, self.enabled)
    }
}

pub fn observability_workflow_step_4(input: &str) -> String {
    let step = ObservabilityWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ObservabilityWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ObservabilityWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("observability:5:{}:{}", self.id, self.enabled)
    }
}

pub fn observability_workflow_step_5(input: &str) -> String {
    let step = ObservabilityWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ObservabilityWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ObservabilityWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("observability:6:{}:{}", self.id, self.enabled)
    }
}

pub fn observability_workflow_step_6(input: &str) -> String {
    let step = ObservabilityWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ObservabilityWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ObservabilityWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("observability:7:{}:{}", self.id, self.enabled)
    }
}

pub fn observability_workflow_step_7(input: &str) -> String {
    let step = ObservabilityWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ObservabilityWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ObservabilityWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("observability:8:{}:{}", self.id, self.enabled)
    }
}

pub fn observability_workflow_step_8(input: &str) -> String {
    let step = ObservabilityWorkflowStep8::new(input);
    step.describe()
}
