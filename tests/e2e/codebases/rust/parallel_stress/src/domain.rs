use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrgId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(pub String);

#[derive(Clone, Debug)]
pub struct UserContext {
    pub user_id: UserId,
    pub org_id: OrgId,
    pub roles: Vec<Role>,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Admin,
    Billing,
    Viewer,
    Support,
}

#[derive(Clone, Debug)]
pub struct ProjectRecord {
    pub project_id: ProjectId,
    pub org_id: OrgId,
    pub name: String,
    pub archived: bool,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct InvoiceRecord {
    pub invoice_id: String,
    pub org_id: OrgId,
    pub account_id: AccountId,
    pub amount_cents: i64,
    pub status: InvoiceStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvoiceStatus {
    Draft,
    Open,
    Paid,
    Void,
}

#[derive(Clone, Debug)]
pub struct MoneyCents {
    pub currency: &'static str,
    pub amount_cents: i64,
}

impl MoneyCents {
    pub fn new(currency: &'static str, amount_cents: i64) -> Self {
        Self {
            currency,
            amount_cents,
        }
    }

    pub fn checked_add(self, other: MoneyCents) -> Option<MoneyCents> {
        if self.currency != other.currency {
            return None;
        }
        Some(MoneyCents {
            currency: self.currency,
            amount_cents: self.amount_cents.checked_add(other.amount_cents)?,
        })
    }

    pub fn checked_discount_bps(self, bps: i64) -> Option<MoneyCents> {
        let discount = self.amount_cents.checked_mul(bps)?.checked_div(10_000)?;
        Some(MoneyCents {
            currency: self.currency,
            amount_cents: self.amount_cents.checked_sub(discount)?,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Pagination {
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Pagination {
    pub fn bounded(limit: usize, cursor: Option<String>) -> Self {
        Self {
            limit: limit.min(100),
            cursor,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RateLimitDecision {
    pub allowed: bool,
    pub remaining: u32,
}

#[derive(Clone, Debug)]
pub struct FeatureFlagDecision {
    pub enabled: bool,
    pub reason: String,
}

pub fn normalize_external_id(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect()
}

pub fn tenant_key(org_id: &OrgId, resource: &str) -> String {
    format!("{}:{}", org_id.0, resource)
}

pub fn display_project(record: &ProjectRecord) -> String {
    format!("{}:{}", record.org_id.0, record.name)
}

#[derive(Clone, Debug)]
pub struct DomainWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DomainWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("domain:1:{}:{}", self.id, self.enabled)
    }
}

pub fn domain_workflow_step_1(input: &str) -> String {
    let step = DomainWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DomainWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DomainWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("domain:2:{}:{}", self.id, self.enabled)
    }
}

pub fn domain_workflow_step_2(input: &str) -> String {
    let step = DomainWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DomainWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DomainWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("domain:3:{}:{}", self.id, self.enabled)
    }
}

pub fn domain_workflow_step_3(input: &str) -> String {
    let step = DomainWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DomainWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DomainWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("domain:4:{}:{}", self.id, self.enabled)
    }
}

pub fn domain_workflow_step_4(input: &str) -> String {
    let step = DomainWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DomainWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DomainWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("domain:5:{}:{}", self.id, self.enabled)
    }
}

pub fn domain_workflow_step_5(input: &str) -> String {
    let step = DomainWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DomainWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DomainWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("domain:6:{}:{}", self.id, self.enabled)
    }
}

pub fn domain_workflow_step_6(input: &str) -> String {
    let step = DomainWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DomainWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DomainWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("domain:7:{}:{}", self.id, self.enabled)
    }
}

pub fn domain_workflow_step_7(input: &str) -> String {
    let step = DomainWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct DomainWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl DomainWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("domain:8:{}:{}", self.id, self.enabled)
    }
}

pub fn domain_workflow_step_8(input: &str) -> String {
    let step = DomainWorkflowStep8::new(input);
    step.describe()
}
