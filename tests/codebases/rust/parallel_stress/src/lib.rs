pub mod access;
pub mod archive;
pub mod common;
pub mod connectors;
pub mod flags;
pub mod gates;
pub mod ledger;
pub mod memo;
pub mod meter;
pub mod notes;
pub mod reports;
pub mod routes;
pub mod rows;
pub mod settings;
pub mod shelves;
pub mod tasks;
pub mod workspace;

pub fn fixture_smoke() -> String {
    let signal = meter::Signal {
        label: "alpha".to_string(),
        name: "speed".to_string(),
        value: 7_500,
    };
    let _ = meter::meter_rule_002(&signal);
    let row = rows::RowRecord {
        group: "alpha".to_string(),
        zone: "north".to_string(),
        item: "sample".to_string(),
        open: true,
    };
    let _ = rows::row_rule_001(&row, "alpha");
    let gate = gates::GateRequest {
        label: "alpha".to_string(),
        amount: 900,
        stamp_count: 2,
    };
    let decision = gates::gate_route_001(&gate);
    format!("{}:{}", signal.label, decision.open)
}
