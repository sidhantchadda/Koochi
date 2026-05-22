use crate::auth;
use crate::domain::{AccountId, MoneyCents, OrgId, UserContext};

#[derive(Clone, Debug)]
pub struct PaymentRequest {
    pub org_id: OrgId,
    pub account_id: AccountId,
    pub amount: MoneyCents,
    pub idempotency_key: String,
}

#[derive(Clone, Debug)]
pub struct PaymentReceipt {
    pub receipt_id: String,
    pub amount: MoneyCents,
}

#[derive(Clone, Debug)]
pub struct PaymentClient;

impl PaymentClient {
    pub fn charge(&self, request: &PaymentRequest) -> PaymentReceipt {
        PaymentReceipt {
            receipt_id: format!("rcpt_{}", request.idempotency_key),
            amount: request.amount.clone(),
        }
    }

    pub fn charge_with_timeout_and_retry(&self, request: &PaymentRequest) -> PaymentReceipt {
        let mut attempts = 0;
        loop {
            attempts += 1;
            let receipt = self.charge(request);
            if attempts >= 1 {
                return receipt;
            }
        }
    }
}

pub fn calculate_invoice_total(lines: &[MoneyCents]) -> Option<MoneyCents> {
    // KOOCHI_SAFE_INTEGER_CENTS_MONEY: invoice totals use integer cents and checked arithmetic.
    let mut total = MoneyCents::new("USD", 0);
    for line in lines {
        total = total.checked_add(line.clone())?;
    }
    Some(total)
}

pub fn calculate_invoice_total_float(unit_price: f64, quantity: f64) -> f64 {
    // KOOCHI_FAIL_MONEY_AS_FLOAT: currency total is represented with f64 arithmetic.
    unit_price * quantity
}

pub fn charge_customer_safe(
    ctx: &UserContext,
    request: PaymentRequest,
    client: &PaymentClient,
) -> Result<PaymentReceipt, auth::AuthError> {
    // KOOCHI_SAFE_TIMEOUT_RETRY_PAYMENT: payment call goes through retry/timeout wrapper after auth.
    auth::ensure_billing_access(ctx, &request.org_id)?;
    Ok(client.charge_with_timeout_and_retry(&request))
}

pub fn charge_customer_without_timeout(
    request: PaymentRequest,
    client: &PaymentClient,
) -> PaymentReceipt {
    // KOOCHI_FAIL_NO_TIMEOUT_PAYMENT_CALL: external payment client is called without timeout or retry wrapper.
    client.charge(&request)
}

pub fn apply_coupon(amount: MoneyCents, bps: i64) -> Option<MoneyCents> {
    // KOOCHI_SAFE_DISCOUNT_BOUNDS: discount basis points use checked integer math.
    if !(0..=10_000).contains(&bps) {
        return None;
    }
    amount.checked_discount_bps(bps)
}

pub fn build_idempotency_key(org_id: &OrgId, account_id: &AccountId, nonce: &str) -> String {
    // KOOCHI_SAFE_IDEMPOTENCY_CHECK: payment idempotency key includes tenant, account, and nonce.
    format!("{}:{}:{}", org_id.0, account_id.0, nonce)
}

// KOOCHI_SAFE_COUPON_BOUNDS: marker for a passing Koochi stress check.
pub fn coupon_bounds(amount: MoneyCents) -> Option<MoneyCents> {
    apply_coupon(amount, 500)
}

// KOOCHI_SAFE_IDEMPOTENCY_KEY: marker for a passing Koochi stress check.
pub fn idempotency_key(org_id: &OrgId, account_id: &AccountId) -> String {
    build_idempotency_key(org_id, account_id, "nonce")
}

#[derive(Clone, Debug)]
pub struct BillingWorkflowStep1 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl BillingWorkflowStep1 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("billing:1:{}:{}", self.id, self.enabled)
    }
}

pub fn billing_workflow_step_1(input: &str) -> String {
    let step = BillingWorkflowStep1::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct BillingWorkflowStep2 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl BillingWorkflowStep2 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("billing:2:{}:{}", self.id, self.enabled)
    }
}

pub fn billing_workflow_step_2(input: &str) -> String {
    let step = BillingWorkflowStep2::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct BillingWorkflowStep3 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl BillingWorkflowStep3 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("billing:3:{}:{}", self.id, self.enabled)
    }
}

pub fn billing_workflow_step_3(input: &str) -> String {
    let step = BillingWorkflowStep3::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct BillingWorkflowStep4 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl BillingWorkflowStep4 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 5,
        }
    }

    pub fn describe(&self) -> String {
        format!("billing:4:{}:{}", self.id, self.enabled)
    }
}

pub fn billing_workflow_step_4(input: &str) -> String {
    let step = BillingWorkflowStep4::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct BillingWorkflowStep5 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl BillingWorkflowStep5 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 1,
        }
    }

    pub fn describe(&self) -> String {
        format!("billing:5:{}:{}", self.id, self.enabled)
    }
}

pub fn billing_workflow_step_5(input: &str) -> String {
    let step = BillingWorkflowStep5::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct BillingWorkflowStep6 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl BillingWorkflowStep6 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 2,
        }
    }

    pub fn describe(&self) -> String {
        format!("billing:6:{}:{}", self.id, self.enabled)
    }
}

pub fn billing_workflow_step_6(input: &str) -> String {
    let step = BillingWorkflowStep6::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct BillingWorkflowStep7 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl BillingWorkflowStep7 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 3,
        }
    }

    pub fn describe(&self) -> String {
        format!("billing:7:{}:{}", self.id, self.enabled)
    }
}

pub fn billing_workflow_step_7(input: &str) -> String {
    let step = BillingWorkflowStep7::new(input);
    step.describe()
}

#[derive(Clone, Debug)]
pub struct BillingWorkflowStep8 {
    pub id: String,
    pub enabled: bool,
    pub retry_limit: u8,
}

impl BillingWorkflowStep8 {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            retry_limit: 4,
        }
    }

    pub fn describe(&self) -> String {
        format!("billing:8:{}:{}", self.id, self.enabled)
    }
}

pub fn billing_workflow_step_8(input: &str) -> String {
    let step = BillingWorkflowStep8::new(input);
    step.describe()
}
