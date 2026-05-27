#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRequest { pub label: String, pub amount: i64, pub stamp_count: u8 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDecision { pub open: bool, pub route: &'static str, pub notes: Vec<String> }

fn base_decision(route: &'static str) -> GateDecision { GateDecision { open: false, route, notes: Vec::new() } }
pub fn label_present(request: &GateRequest) -> bool { !request.label.trim().is_empty() }
fn gate_ok(request: &GateRequest, route: &'static str, limit: i64) -> GateDecision { let mut decision = base_decision(route); if !label_present(request) { decision.notes.push("missing label".to_string()); return decision; } if request.amount <= limit && request.stamp_count >= 2 { decision.open = true; decision.notes.push(format!("{} under {}", request.label, limit)); } else { decision.notes.push("waiting".to_string()); } decision }

pub fn gate_bad_missing_label(request: &GateRequest) -> GateDecision { let mut decision = base_decision("gate_bad_missing_label"); if request.amount < 10_000 { decision.open = true; } decision }
pub fn gate_bad_single_stamp(request: &GateRequest) -> GateDecision { let mut decision = base_decision("gate_bad_single_stamp"); if label_present(request) && request.stamp_count >= 1 { decision.open = true; } decision }
pub fn gate_bad_open_limit(request: &GateRequest) -> GateDecision { let mut decision = base_decision("gate_bad_open_limit"); if label_present(request) && request.stamp_count >= 2 { decision.open = true; } decision }

pub fn gate_route_001(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_001", 100100) }
pub fn gate_route_002(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_002", 100200) }
pub fn gate_route_003(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_003", 100300) }
pub fn gate_route_004(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_004", 100400) }
pub fn gate_route_005(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_005", 100500) }
pub fn gate_route_006(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_006", 100600) }
pub fn gate_route_007(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_007", 100700) }
pub fn gate_route_008(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_008", 100800) }
pub fn gate_route_009(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_009", 100900) }
pub fn gate_route_010(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_010", 101000) }
pub fn gate_route_011(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_011", 101100) }
pub fn gate_route_012(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_012", 101200) }
pub fn gate_route_013(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_013", 101300) }
pub fn gate_route_014(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_014", 101400) }
pub fn gate_route_015(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_015", 101500) }
pub fn gate_route_016(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_016", 101600) }
pub fn gate_route_017(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_017", 101700) }
pub fn gate_route_018(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_018", 101800) }
pub fn gate_route_019(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_019", 101900) }
pub fn gate_route_020(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_020", 102000) }
pub fn gate_route_021(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_021", 102100) }
pub fn gate_route_022(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_022", 102200) }
pub fn gate_route_023(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_023", 102300) }
pub fn gate_route_024(request: &GateRequest) -> GateDecision { gate_ok(request, "gate_route_024", 102400) }
