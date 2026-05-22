use crate::domain::{tenant_key, OrgId};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct RateCard {
    pub org_id: OrgId,
    pub version: u64,
    pub entries: Vec<String>,
}

#[derive(Default)]
pub struct SingleFlightCache {
    values: HashMap<String, RateCard>,
    in_flight: HashSet<String>,
}

impl SingleFlightCache {
    pub fn get_or_load<F>(&mut self, org_id: &OrgId, loader: F) -> RateCard
    where
        F: FnOnce() -> RateCard,
    {
        // KOOCHI_SAFE_SINGLE_FLIGHT_CACHE: cache miss is protected by an in-flight guard.
        let key = tenant_key(org_id, "rate_card");
        if let Some(value) = self.values.get(&key) {
            return value.clone();
        }
        if self.in_flight.contains(&key) {
            return RateCard {
                org_id: org_id.clone(),
                version: 0,
                entries: vec![],
            };
        }
        self.in_flight.insert(key.clone());
        let loaded = loader();
        self.values.insert(key.clone(), loaded.clone());
        self.in_flight.remove(&key);
        loaded
    }

    pub fn invalidate(&mut self, org_id: &OrgId) {
        self.values.remove(&tenant_key(org_id, "rate_card"));
    }
}

#[derive(Default)]
pub struct NaiveCache {
    values: HashMap<String, RateCard>,
}

impl NaiveCache {
    pub fn get_or_load<F>(&mut self, org_id: &OrgId, loader: F) -> RateCard
    where
        F: FnOnce() -> RateCard,
    {
        // KOOCHI_FAIL_CACHE_STAMPEDE: expensive loader runs on every miss without single-flight guard.
        let key = tenant_key(org_id, "rate_card");
        if !self.values.contains_key(&key) {
            let loaded = loader();
            self.values.insert(key.clone(), loaded);
        }
        self.values.get(&key).cloned().unwrap()
    }
}

pub fn cache_key_for_feature(org_id: &OrgId, feature: &str) -> String {
    // KOOCHI_SAFE_TENANT_CACHE_KEY: cache key includes tenant id and normalized feature name.
    format!(
        "feature:{}:{}",
        org_id.0,
        feature.trim().to_ascii_lowercase()
    )
}

pub fn cache_ttl_seconds(resource: &str) -> u64 {
    match resource {
        "rate_card" => 60,
        "feature" => 30,
        "report" => 300,
        _ => 15,
    }
}

// KOOCHI_SAFE_CACHE_TTL: marker for a passing Koochi stress check.
pub fn cache_ttl() -> u64 {
    cache_ttl_seconds("report")
}

// KOOCHI_SAFE_FEATURE_CACHE_KEY: marker for a passing Koochi stress check.
pub fn feature_cache_key(org_id: &OrgId) -> String {
    cache_key_for_feature(org_id, "Launch")
}

#[derive(Clone, Debug)]
pub struct CacheWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl CacheWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("cache:1:{}:{}", self.id, self.enabled)
    }
}

pub fn cache_workflow_step_1(input: &str) -> String {
    let step = CacheWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct CacheWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl CacheWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("cache:2:{}:{}", self.id, self.enabled)
    }
}

pub fn cache_workflow_step_2(input: &str) -> String {
    let step = CacheWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct CacheWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl CacheWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("cache:3:{}:{}", self.id, self.enabled)
    }
}

pub fn cache_workflow_step_3(input: &str) -> String {
    let step = CacheWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct CacheWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl CacheWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("cache:4:{}:{}", self.id, self.enabled)
    }
}

pub fn cache_workflow_step_4(input: &str) -> String {
    let step = CacheWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct CacheWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl CacheWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("cache:5:{}:{}", self.id, self.enabled)
    }
}

pub fn cache_workflow_step_5(input: &str) -> String {
    let step = CacheWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct CacheWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl CacheWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("cache:6:{}:{}", self.id, self.enabled)
    }
}

pub fn cache_workflow_step_6(input: &str) -> String {
    let step = CacheWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct CacheWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl CacheWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("cache:7:{}:{}", self.id, self.enabled)
    }
}

pub fn cache_workflow_step_7(input: &str) -> String {
    let step = CacheWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct CacheWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl CacheWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("cache:8:{}:{}", self.id, self.enabled)
    }
}

pub fn cache_workflow_step_8(input: &str) -> String {
    let step = CacheWorkflowStep8::new(input);
    step.describe()
}
