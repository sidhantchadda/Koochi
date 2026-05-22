use crate::domain::{OrgId, ProjectRecord, Role, UserContext};

#[derive(Debug, Clone)]
pub enum AuthError {
    Forbidden,
    MissingScope,
    WrongTenant,
}

#[derive(Debug, Clone)]
pub struct PermissionSet {
    pub can_read_project: bool,
    pub can_update_billing: bool,
    pub can_export_reports: bool,
    pub can_manage_jobs: bool,
}

impl PermissionSet {
    pub fn from_context(ctx: &UserContext) -> Self {
        let admin = ctx
            .roles
            .iter()
            .any(|role| matches!(role, Role::Owner | Role::Admin));
        let billing = ctx.roles.iter().any(|role| matches!(role, Role::Billing));
        Self {
            can_read_project: admin
                || ctx
                    .roles
                    .iter()
                    .any(|role| matches!(role, Role::Viewer | Role::Support)),
            can_update_billing: admin || billing,
            can_export_reports: admin || ctx.scopes.iter().any(|scope| scope == "reports:export"),
            can_manage_jobs: admin || ctx.scopes.iter().any(|scope| scope == "jobs:manage"),
        }
    }
}

pub fn ensure_same_org(ctx: &UserContext, org_id: &OrgId) -> Result<(), AuthError> {
    if &ctx.org_id == org_id {
        Ok(())
    } else {
        Err(AuthError::WrongTenant)
    }
}

pub fn ensure_project_access(ctx: &UserContext, project: &ProjectRecord) -> Result<(), AuthError> {
    // KOOCHI_SAFE_AUTHORIZATION_GUARD: validates tenant ownership before allowing project access.
    ensure_same_org(ctx, &project.org_id)?;
    let permissions = PermissionSet::from_context(ctx);
    if permissions.can_read_project {
        Ok(())
    } else {
        Err(AuthError::Forbidden)
    }
}

pub fn ensure_billing_access(ctx: &UserContext, org_id: &OrgId) -> Result<(), AuthError> {
    // KOOCHI_SAFE_BILLING_AUTHORIZATION: billing changes require same tenant plus billing permission.
    ensure_same_org(ctx, org_id)?;
    if PermissionSet::from_context(ctx).can_update_billing {
        Ok(())
    } else {
        Err(AuthError::Forbidden)
    }
}

pub fn ensure_report_export(ctx: &UserContext, org_id: &OrgId) -> Result<(), AuthError> {
    // KOOCHI_SAFE_REPORT_AUTHORIZATION: report exports are checked against org and export scope.
    ensure_same_org(ctx, org_id)?;
    if PermissionSet::from_context(ctx).can_export_reports {
        Ok(())
    } else {
        Err(AuthError::MissingScope)
    }
}

pub fn ensure_job_management(ctx: &UserContext, org_id: &OrgId) -> Result<(), AuthError> {
    // KOOCHI_SAFE_JOB_AUTHORIZATION: job management is tenant scoped and permission checked.
    ensure_same_org(ctx, org_id)?;
    if PermissionSet::from_context(ctx).can_manage_jobs {
        Ok(())
    } else {
        Err(AuthError::Forbidden)
    }
}

pub fn insecure_project_lookup(project: &ProjectRecord) -> String {
    // KOOCHI_FAIL_MISSING_ORG_AUTH: exposes a project without checking caller org or permissions.
    format!("project:{}", project.project_id.0)
}

pub fn support_can_view(ctx: &UserContext, org_id: &OrgId) -> bool {
    ensure_same_org(ctx, org_id).is_ok()
        && ctx
            .roles
            .iter()
            .any(|role| matches!(role, Role::Support | Role::Admin | Role::Owner))
}

// KOOCHI_SAFE_SUPPORT_ROLE_CHECK: marker for a passing Koochi stress check.
pub fn support_role_check(ctx: &UserContext, org_id: &OrgId) -> bool {
    support_can_view(ctx, org_id)
}

// KOOCHI_SAFE_SCOPE_CHECK: marker for a passing Koochi stress check.
pub fn scope_check(ctx: &UserContext, scope: &str) -> bool {
    ctx.scopes.iter().any(|candidate| candidate == scope)
}

#[derive(Clone, Debug)]
pub struct AuthWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl AuthWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("auth:1:{}:{}", self.id, self.enabled)
    }
}

pub fn auth_workflow_step_1(input: &str) -> String {
    let step = AuthWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct AuthWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl AuthWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("auth:2:{}:{}", self.id, self.enabled)
    }
}

pub fn auth_workflow_step_2(input: &str) -> String {
    let step = AuthWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct AuthWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl AuthWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("auth:3:{}:{}", self.id, self.enabled)
    }
}

pub fn auth_workflow_step_3(input: &str) -> String {
    let step = AuthWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct AuthWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl AuthWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("auth:4:{}:{}", self.id, self.enabled)
    }
}

pub fn auth_workflow_step_4(input: &str) -> String {
    let step = AuthWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct AuthWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl AuthWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("auth:5:{}:{}", self.id, self.enabled)
    }
}

pub fn auth_workflow_step_5(input: &str) -> String {
    let step = AuthWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct AuthWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl AuthWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("auth:6:{}:{}", self.id, self.enabled)
    }
}

pub fn auth_workflow_step_6(input: &str) -> String {
    let step = AuthWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct AuthWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl AuthWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("auth:7:{}:{}", self.id, self.enabled)
    }
}

pub fn auth_workflow_step_7(input: &str) -> String {
    let step = AuthWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct AuthWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl AuthWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("auth:8:{}:{}", self.id, self.enabled)
    }
}

pub fn auth_workflow_step_8(input: &str) -> String {
    let step = AuthWorkflowStep8::new(input);
    step.describe()
}
