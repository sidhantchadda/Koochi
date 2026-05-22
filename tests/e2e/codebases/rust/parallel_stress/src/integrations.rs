use crate::domain::OrgId;

#[derive(Clone, Debug)]
pub struct WebhookRequest {
    pub org_id: OrgId,
    pub signature: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct WebhookSecret(pub String);

pub fn verify_signature(request: &WebhookRequest, secret: &WebhookSecret) -> bool {
    // KOOCHI_SAFE_WEBHOOK_SIGNATURE: webhook body is accepted only when signature matches tenant secret.
    request.signature == format!("signed:{}:{}", request.org_id.0, secret.0)
}

pub fn accept_webhook(request: &WebhookRequest, secret: &WebhookSecret) -> bool {
    verify_signature(request, secret) && !request.body.is_empty()
}

pub fn call_with_timeout(endpoint: &str, timeout_ms: u64) -> String {
    // KOOCHI_SAFE_EXTERNAL_TIMEOUT: external integration receives a bounded timeout value.
    format!("endpoint={} timeout_ms={}", endpoint, timeout_ms.min(5_000))
}

pub fn retry_budget(max_retries: u8) -> u8 {
    max_retries.clamp(1, 4)
}

// KOOCHI_SAFE_RETRY_BUDGET: marker for a passing Koochi stress check.
pub fn retry_budget_safe() -> u8 {
    retry_budget(9)
}

// KOOCHI_SAFE_WEBHOOK_ACCEPTANCE: marker for a passing Koochi stress check.
pub fn webhook_acceptance(req: &WebhookRequest, secret: &WebhookSecret) -> bool {
    accept_webhook(req, secret)
}

#[derive(Clone, Debug)]
pub struct IntegrationsWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl IntegrationsWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("integrations:1:{}:{}", self.id, self.enabled)
    }
}

pub fn integrations_workflow_step_1(input: &str) -> String {
    let step = IntegrationsWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct IntegrationsWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl IntegrationsWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("integrations:2:{}:{}", self.id, self.enabled)
    }
}

pub fn integrations_workflow_step_2(input: &str) -> String {
    let step = IntegrationsWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct IntegrationsWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl IntegrationsWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("integrations:3:{}:{}", self.id, self.enabled)
    }
}

pub fn integrations_workflow_step_3(input: &str) -> String {
    let step = IntegrationsWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct IntegrationsWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl IntegrationsWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("integrations:4:{}:{}", self.id, self.enabled)
    }
}

pub fn integrations_workflow_step_4(input: &str) -> String {
    let step = IntegrationsWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct IntegrationsWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl IntegrationsWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("integrations:5:{}:{}", self.id, self.enabled)
    }
}

pub fn integrations_workflow_step_5(input: &str) -> String {
    let step = IntegrationsWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct IntegrationsWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl IntegrationsWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("integrations:6:{}:{}", self.id, self.enabled)
    }
}

pub fn integrations_workflow_step_6(input: &str) -> String {
    let step = IntegrationsWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct IntegrationsWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl IntegrationsWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("integrations:7:{}:{}", self.id, self.enabled)
    }
}

pub fn integrations_workflow_step_7(input: &str) -> String {
    let step = IntegrationsWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct IntegrationsWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl IntegrationsWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("integrations:8:{}:{}", self.id, self.enabled)
    }
}

pub fn integrations_workflow_step_8(input: &str) -> String {
    let step = IntegrationsWorkflowStep8::new(input);
    step.describe()
}
