use crate::domain::{FeatureFlagDecision, OrgId};

pub fn safe_feature_flag(
    org_id: &OrgId,
    feature: &str,
    rollout_percent: u8,
) -> FeatureFlagDecision {
    // KOOCHI_SAFE_FEATURE_FLAG: rollout is bounded and tenant scoped.
    let bounded = rollout_percent.min(100);
    let bucket = org_id.0.bytes().fold(0_u32, |acc, byte| acc + byte as u32) % 100;
    FeatureFlagDecision {
        enabled: bucket < bounded as u32,
        reason: format!("feature={} rollout={}", feature, bounded),
    }
}

pub fn disabled_feature(feature: &str) -> FeatureFlagDecision {
    FeatureFlagDecision {
        enabled: false,
        reason: format!("{} disabled", feature),
    }
}

// KOOCHI_SAFE_DISABLED_FEATURE: marker for a passing Koochi stress check.
pub fn disabled_feature_safe() -> FeatureFlagDecision {
    disabled_feature("legacy")
}

#[derive(Clone, Debug)]
pub struct FeaturesWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl FeaturesWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("features:1:{}:{}", self.id, self.enabled)
    }
}

pub fn features_workflow_step_1(input: &str) -> String {
    let step = FeaturesWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct FeaturesWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl FeaturesWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("features:2:{}:{}", self.id, self.enabled)
    }
}

pub fn features_workflow_step_2(input: &str) -> String {
    let step = FeaturesWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct FeaturesWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl FeaturesWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("features:3:{}:{}", self.id, self.enabled)
    }
}

pub fn features_workflow_step_3(input: &str) -> String {
    let step = FeaturesWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct FeaturesWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl FeaturesWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("features:4:{}:{}", self.id, self.enabled)
    }
}

pub fn features_workflow_step_4(input: &str) -> String {
    let step = FeaturesWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct FeaturesWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl FeaturesWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("features:5:{}:{}", self.id, self.enabled)
    }
}

pub fn features_workflow_step_5(input: &str) -> String {
    let step = FeaturesWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct FeaturesWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl FeaturesWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("features:6:{}:{}", self.id, self.enabled)
    }
}

pub fn features_workflow_step_6(input: &str) -> String {
    let step = FeaturesWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct FeaturesWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl FeaturesWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("features:7:{}:{}", self.id, self.enabled)
    }
}

pub fn features_workflow_step_7(input: &str) -> String {
    let step = FeaturesWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct FeaturesWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl FeaturesWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("features:8:{}:{}", self.id, self.enabled)
    }
}

pub fn features_workflow_step_8(input: &str) -> String {
    let step = FeaturesWorkflowStep8::new(input);
    step.describe()
}
