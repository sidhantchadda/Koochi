use crate::domain::{OrgId, Pagination, ProjectId};

#[derive(Clone, Debug)]
pub struct Query {
    pub sql: String,
    pub params: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ProjectRow {
    pub project_id: String,
    pub org_id: String,
    pub name: String,
}

pub trait Db {
    fn fetch_all(&self, query: Query) -> Vec<ProjectRow>;
    fn fetch_one(&self, query: Query) -> Option<ProjectRow>;
}

pub fn project_by_id_query(org_id: &OrgId, project_id: &ProjectId) -> Query {
    // KOOCHI_SAFE_PARAMETERIZED_SQL: org_id and project_id are bound as query parameters.
    Query {
        sql: "select * from projects where org_id = $1 and project_id = $2".to_string(),
        params: vec![org_id.0.clone(), project_id.0.clone()],
    }
}

pub fn invoice_search_query(org_id: &OrgId, status: &str, pagination: Pagination) -> Query {
    // KOOCHI_SAFE_TENANT_SCOPED_QUERY: invoice search includes org_id and bounded limit.
    Query {
        sql: "select * from invoices where org_id = $1 and status = $2 limit $3".to_string(),
        params: vec![
            org_id.0.clone(),
            status.to_string(),
            pagination.limit.to_string(),
        ],
    }
}

pub fn unsafe_invoice_lookup(org_id: &str, invoice_id: &str) -> Query {
    // KOOCHI_FAIL_SQL_INTERPOLATION: user-controlled org_id and invoice_id are interpolated into SQL.
    Query {
        sql: format!(
            "select * from invoices where org_id = '{}' and invoice_id = '{}'",
            org_id, invoice_id
        ),
        params: vec![],
    }
}

pub fn audit_events_query(org_id: &OrgId, pagination: Pagination) -> Query {
    // KOOCHI_SAFE_PAGINATION_LIMIT: user supplied limits are clamped before query construction.
    let page = Pagination::bounded(pagination.limit, pagination.cursor);
    Query {
        sql: "select * from audit_events where org_id = $1 order by created_at desc limit $2"
            .to_string(),
        params: vec![org_id.0.clone(), page.limit.to_string()],
    }
}

pub fn tenant_usage_query(org_id: &OrgId) -> Query {
    Query {
        sql: "select metric, value from usage where org_id = $1".to_string(),
        params: vec![org_id.0.clone()],
    }
}

pub fn insert_idempotency_key(org_id: &OrgId, key: &str) -> Query {
    // KOOCHI_SAFE_IDEMPOTENCY_STORAGE: idempotency keys are tenant scoped and inserted with parameters.
    Query {
        sql: "insert into idempotency_keys(org_id, key) values($1, $2) on conflict do nothing"
            .to_string(),
        params: vec![org_id.0.clone(), key.to_string()],
    }
}

pub fn delete_expired_sessions(org_id: &OrgId) -> Query {
    Query {
        sql: "delete from sessions where org_id = $1 and expires_at < now()".to_string(),
        params: vec![org_id.0.clone()],
    }
}

// KOOCHI_SAFE_AUDIT_PARAMETERIZED_SQL: marker for a passing Koochi stress check.
pub fn audit_parameterized_sql(org_id: &OrgId) -> Query {
    audit_events_query(org_id, Pagination::bounded(50, None))
}

// KOOCHI_SAFE_DELETE_TENANT_SCOPED: marker for a passing Koochi stress check.
pub fn delete_tenant_scoped(org_id: &OrgId) -> Query {
    delete_expired_sessions(org_id)
}

#[derive(Clone, Debug)]
pub struct StorageWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl StorageWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("storage:1:{}:{}", self.id, self.enabled)
    }
}

pub fn storage_workflow_step_1(input: &str) -> String {
    let step = StorageWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct StorageWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl StorageWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("storage:2:{}:{}", self.id, self.enabled)
    }
}

pub fn storage_workflow_step_2(input: &str) -> String {
    let step = StorageWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct StorageWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl StorageWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("storage:3:{}:{}", self.id, self.enabled)
    }
}

pub fn storage_workflow_step_3(input: &str) -> String {
    let step = StorageWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct StorageWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl StorageWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("storage:4:{}:{}", self.id, self.enabled)
    }
}

pub fn storage_workflow_step_4(input: &str) -> String {
    let step = StorageWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct StorageWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl StorageWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("storage:5:{}:{}", self.id, self.enabled)
    }
}

pub fn storage_workflow_step_5(input: &str) -> String {
    let step = StorageWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct StorageWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl StorageWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("storage:6:{}:{}", self.id, self.enabled)
    }
}

pub fn storage_workflow_step_6(input: &str) -> String {
    let step = StorageWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct StorageWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl StorageWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("storage:7:{}:{}", self.id, self.enabled)
    }
}

pub fn storage_workflow_step_7(input: &str) -> String {
    let step = StorageWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct StorageWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl StorageWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("storage:8:{}:{}", self.id, self.enabled)
    }
}

pub fn storage_workflow_step_8(input: &str) -> String {
    let step = StorageWorkflowStep8::new(input);
    step.describe()
}
