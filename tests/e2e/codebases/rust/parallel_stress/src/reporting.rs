use crate::auth;
use crate::domain::{OrgId, UserContext};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ReportRequest {
    pub org_id: OrgId,
    pub report_name: String,
}

pub fn sanitize_report_name(name: &str) -> Option<String> {
    let clean: String = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

pub fn safe_report_path(base: &Path, name: &str) -> Option<PathBuf> {
    // KOOCHI_SAFE_PATH_ALLOWLIST: report file names are allowlisted before joining with base path.
    let clean = sanitize_report_name(name)?;
    Some(base.join(format!("{clean}.json")))
}

pub fn unsafe_report_path(base: &Path, name: &str) -> PathBuf {
    // KOOCHI_FAIL_PATH_TRAVERSAL: user controlled report name is joined without validation.
    base.join(name)
}

pub fn export_report(
    ctx: &UserContext,
    request: ReportRequest,
    base: &Path,
) -> Result<PathBuf, auth::AuthError> {
    // KOOCHI_SAFE_FILE_EXPORT: export checks tenant authorization and uses sanitized path construction.
    auth::ensure_report_export(ctx, &request.org_id)?;
    Ok(safe_report_path(base, &request.report_name).unwrap_or_else(|| base.join("empty.json")))
}

pub fn report_manifest(org_id: &OrgId, names: &[String]) -> Vec<String> {
    names
        .iter()
        .map(|name| format!("{}:{}", org_id.0, name))
        .collect()
}

// KOOCHI_SAFE_REPORT_NAME_SANITIZER: marker for a passing Koochi stress check.
pub fn report_name_sanitizer(name: &str) -> Option<String> {
    sanitize_report_name(name)
}

// KOOCHI_SAFE_REPORT_MANIFEST_TENANT: marker for a passing Koochi stress check.
pub fn report_manifest_tenant(org_id: &OrgId) -> Vec<String> {
    report_manifest(org_id, &["daily".to_string()])
}

#[derive(Clone, Debug)]
pub struct ReportingWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ReportingWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("reporting:1:{}:{}", self.id, self.enabled)
    }
}

pub fn reporting_workflow_step_1(input: &str) -> String {
    let step = ReportingWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ReportingWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ReportingWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("reporting:2:{}:{}", self.id, self.enabled)
    }
}

pub fn reporting_workflow_step_2(input: &str) -> String {
    let step = ReportingWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ReportingWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ReportingWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("reporting:3:{}:{}", self.id, self.enabled)
    }
}

pub fn reporting_workflow_step_3(input: &str) -> String {
    let step = ReportingWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ReportingWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ReportingWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("reporting:4:{}:{}", self.id, self.enabled)
    }
}

pub fn reporting_workflow_step_4(input: &str) -> String {
    let step = ReportingWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ReportingWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ReportingWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("reporting:5:{}:{}", self.id, self.enabled)
    }
}

pub fn reporting_workflow_step_5(input: &str) -> String {
    let step = ReportingWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ReportingWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ReportingWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("reporting:6:{}:{}", self.id, self.enabled)
    }
}

pub fn reporting_workflow_step_6(input: &str) -> String {
    let step = ReportingWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ReportingWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ReportingWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("reporting:7:{}:{}", self.id, self.enabled)
    }
}

pub fn reporting_workflow_step_7(input: &str) -> String {
    let step = ReportingWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct ReportingWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl ReportingWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("reporting:8:{}:{}", self.id, self.enabled)
    }
}

pub fn reporting_workflow_step_8(input: &str) -> String {
    let step = ReportingWorkflowStep8::new(input);
    step.describe()
}
