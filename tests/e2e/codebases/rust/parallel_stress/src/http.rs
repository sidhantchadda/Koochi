use crate::auth;
use crate::domain::{OrgId, Pagination, ProjectId, ProjectRecord, RequestId, UserContext};
use crate::observability::{log_request_safe, RequestLog};
use crate::storage::{project_by_id_query, Query};

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub request_id: RequestId,
    pub org_id: OrgId,
    pub project_id: Option<ProjectId>,
    pub authorization: Option<String>,
    pub cookie: Option<String>,
    pub route: String,
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub fn get_project(
    ctx: &UserContext,
    req: HttpRequest,
    project: ProjectRecord,
) -> Result<HttpResponse, auth::AuthError> {
    // KOOCHI_SAFE_HTTP_AUTH_FLOW: handler checks project access before returning project data.
    auth::ensure_project_access(ctx, &project)?;
    let _log = log_request_safe(RequestLog {
        request_id: &req.request_id,
        org_id: &req.org_id,
        route: &req.route,
        authorization: req.authorization.as_deref(),
        api_key: None,
        cookie: req.cookie.as_deref(),
    });
    Ok(HttpResponse {
        status: 200,
        body: project.name,
    })
}

pub fn list_projects_query(
    ctx: &UserContext,
    req: HttpRequest,
    pagination: Pagination,
) -> Result<Query, auth::AuthError> {
    // KOOCHI_SAFE_LIST_AUTH_AND_PAGINATION: list endpoint checks tenant and clamps pagination.
    auth::ensure_same_org(ctx, &req.org_id)?;
    Ok(Query {
        sql: "select * from projects where org_id = $1 limit $2".to_string(),
        params: vec![
            req.org_id.0,
            Pagination::bounded(pagination.limit, pagination.cursor)
                .limit
                .to_string(),
        ],
    })
}

pub fn project_detail_query(req: &HttpRequest) -> Option<Query> {
    let project_id = req.project_id.as_ref()?;
    Some(project_by_id_query(&req.org_id, project_id))
}

pub fn health_check() -> HttpResponse {
    HttpResponse {
        status: 200,
        body: "ok".to_string(),
    }
}

pub fn parse_org_header(value: &str) -> OrgId {
    OrgId(value.trim().to_string())
}

// KOOCHI_SAFE_HEALTH_NO_SECRET: marker for a passing Koochi stress check.
pub fn health_no_secret() -> HttpResponse {
    health_check()
}

// KOOCHI_SAFE_ORG_HEADER_PARSE: marker for a passing Koochi stress check.
pub fn org_header_parse(value: &str) -> OrgId {
    parse_org_header(value)
}

#[derive(Clone, Debug)]
pub struct HttpWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl HttpWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("http:1:{}:{}", self.id, self.enabled)
    }
}

pub fn http_workflow_step_1(input: &str) -> String {
    let step = HttpWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct HttpWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl HttpWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("http:2:{}:{}", self.id, self.enabled)
    }
}

pub fn http_workflow_step_2(input: &str) -> String {
    let step = HttpWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct HttpWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl HttpWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("http:3:{}:{}", self.id, self.enabled)
    }
}

pub fn http_workflow_step_3(input: &str) -> String {
    let step = HttpWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct HttpWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl HttpWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("http:4:{}:{}", self.id, self.enabled)
    }
}

pub fn http_workflow_step_4(input: &str) -> String {
    let step = HttpWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct HttpWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl HttpWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("http:5:{}:{}", self.id, self.enabled)
    }
}

pub fn http_workflow_step_5(input: &str) -> String {
    let step = HttpWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct HttpWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl HttpWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("http:6:{}:{}", self.id, self.enabled)
    }
}

pub fn http_workflow_step_6(input: &str) -> String {
    let step = HttpWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct HttpWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl HttpWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("http:7:{}:{}", self.id, self.enabled)
    }
}

pub fn http_workflow_step_7(input: &str) -> String {
    let step = HttpWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct HttpWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl HttpWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("http:8:{}:{}", self.id, self.enabled)
    }
}

pub fn http_workflow_step_8(input: &str) -> String {
    let step = HttpWorkflowStep8::new(input);
    step.describe()
}
