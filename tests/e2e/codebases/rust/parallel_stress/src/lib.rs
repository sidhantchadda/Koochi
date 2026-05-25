pub mod analytics;
pub mod compliance;
pub mod workflows;
pub mod auth;
pub mod billing;
pub mod cache;
pub mod config;
pub mod dead_code;
pub mod domain;
pub mod features;
pub mod http;
pub mod integrations;
pub mod jobs;
pub mod observability;
pub mod reporting;
pub mod storage;
pub mod tenant;

use domain::{AccountId, MoneyCents, OrgId};

pub fn fixture_smoke() -> String {
    let org = OrgId("org_parallel".to_string());
    let account = AccountId("acct_parallel".to_string());
    let key = billing::build_idempotency_key(&org, &account, "startup");
    let amount = MoneyCents::new("USD", 1200);
    let _ = billing::apply_coupon(amount, 250);
    key
}
