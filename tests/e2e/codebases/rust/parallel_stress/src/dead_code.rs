use crate::domain::OrgId;

pub fn referenced_reconciliation_helper(org_id: &OrgId) -> String {
    // KOOCHI_SAFE_REFERENCED_HELPER: this helper is referenced by the reconciliation plan.
    format!("reconcile:{}", org_id.0)
}

fn abandoned_enterprise_migration(org_id: &OrgId) -> String {
    // KOOCHI_FAIL_DEAD_CODE: this private migration helper has no callers in the fixture.
    format!("legacy-migration:{}", org_id.0)
}

pub fn reconciliation_plan(org_id: &OrgId) -> Vec<String> {
    vec![referenced_reconciliation_helper(org_id)]
}

#[derive(Clone, Debug)]
pub struct DeadCodeWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DeadCodeWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("dead_code:1:{}:{}", self.id, self.enabled)
    }
}

pub fn dead_code_workflow_step_1(input: &str) -> String {
    let step = DeadCodeWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DeadCodeWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DeadCodeWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("dead_code:2:{}:{}", self.id, self.enabled)
    }
}

pub fn dead_code_workflow_step_2(input: &str) -> String {
    let step = DeadCodeWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DeadCodeWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DeadCodeWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("dead_code:3:{}:{}", self.id, self.enabled)
    }
}

pub fn dead_code_workflow_step_3(input: &str) -> String {
    let step = DeadCodeWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DeadCodeWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DeadCodeWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("dead_code:4:{}:{}", self.id, self.enabled)
    }
}

pub fn dead_code_workflow_step_4(input: &str) -> String {
    let step = DeadCodeWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DeadCodeWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DeadCodeWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("dead_code:5:{}:{}", self.id, self.enabled)
    }
}

pub fn dead_code_workflow_step_5(input: &str) -> String {
    let step = DeadCodeWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DeadCodeWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DeadCodeWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("dead_code:6:{}:{}", self.id, self.enabled)
    }
}

pub fn dead_code_workflow_step_6(input: &str) -> String {
    let step = DeadCodeWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DeadCodeWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DeadCodeWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("dead_code:7:{}:{}", self.id, self.enabled)
    }
}

pub fn dead_code_workflow_step_7(input: &str) -> String {
    let step = DeadCodeWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DeadCodeWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DeadCodeWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("dead_code:8:{}:{}", self.id, self.enabled)
    }
}

pub fn dead_code_workflow_step_8(input: &str) -> String {
    let step = DeadCodeWorkflowStep8::new(input);
    step.describe()
}
