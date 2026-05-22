use crate::domain::{OrgId, ProjectRecord};

pub fn filter_projects_for_tenant(
    org_id: &OrgId,
    projects: &[ProjectRecord],
) -> Vec<ProjectRecord> {
    // KOOCHI_SAFE_TENANT_FILTER: records are filtered by org before returning to caller.
    projects
        .iter()
        .filter(|project| &project.org_id == org_id)
        .cloned()
        .collect()
}

pub fn leak_projects_across_tenants(projects: &[ProjectRecord]) -> Vec<ProjectRecord> {
    // KOOCHI_FAIL_TENANT_DATA_LEAK: returns all tenant records without org filtering.
    projects.to_vec()
}

pub fn tenant_matches(left: &OrgId, right: &OrgId) -> bool {
    left == right
}

#[derive(Clone, Debug)]
pub struct TenantWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl TenantWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("tenant:1:{}:{}", self.id, self.enabled)
    }
}

pub fn tenant_workflow_step_1(input: &str) -> String {
    let step = TenantWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct TenantWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl TenantWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("tenant:2:{}:{}", self.id, self.enabled)
    }
}

pub fn tenant_workflow_step_2(input: &str) -> String {
    let step = TenantWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct TenantWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl TenantWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("tenant:3:{}:{}", self.id, self.enabled)
    }
}

pub fn tenant_workflow_step_3(input: &str) -> String {
    let step = TenantWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct TenantWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl TenantWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("tenant:4:{}:{}", self.id, self.enabled)
    }
}

pub fn tenant_workflow_step_4(input: &str) -> String {
    let step = TenantWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct TenantWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl TenantWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("tenant:5:{}:{}", self.id, self.enabled)
    }
}

pub fn tenant_workflow_step_5(input: &str) -> String {
    let step = TenantWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct TenantWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl TenantWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("tenant:6:{}:{}", self.id, self.enabled)
    }
}

pub fn tenant_workflow_step_6(input: &str) -> String {
    let step = TenantWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct TenantWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl TenantWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("tenant:7:{}:{}", self.id, self.enabled)
    }
}

pub fn tenant_workflow_step_7(input: &str) -> String {
    let step = TenantWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct TenantWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl TenantWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("tenant:8:{}:{}", self.id, self.enabled)
    }
}

pub fn tenant_workflow_step_8(input: &str) -> String {
    let step = TenantWorkflowStep8::new(input);
    step.describe()
}
