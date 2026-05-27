#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal { pub label: String, pub name: String, pub value: i64 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeterItem { pub rule: &'static str, pub level: &'static str, pub note: String }

pub fn normalize_name(raw: &str) -> String { raw.trim().to_ascii_lowercase().replace(' ', "_") }
pub fn bounded_value(signal: &Signal) -> i64 { signal.value.clamp(0, 10_000) }
fn meter_item(rule: &'static str, level: &'static str, note: String) -> MeterItem { MeterItem { rule, level, note } }
fn meter_ok(signal: &Signal, rule: &'static str, threshold: i64) -> Option<MeterItem> { let name = normalize_name(&signal.name); let bounded = bounded_value(signal); if signal.label.trim().is_empty() || name == "paused" { return None; } if bounded > threshold { Some(meter_item(rule, if bounded > 9000 { "red" } else { "amber" }, format!("{}:{}:{}", signal.label, name, bounded))) } else { None } }

pub fn meter_bad_empty_label(signal: &Signal) -> Option<MeterItem> { if signal.value > 12_000 { Some(meter_item("meter_bad_empty_label", "red", format!("{} accepted", signal.name))) } else { None } }
pub fn meter_bad_open_scale(signal: &Signal) -> i64 { signal.value * 100 }
pub fn meter_bad_paused_name(signal: &Signal) -> Option<MeterItem> { if normalize_name(&signal.name) == "paused" && signal.value > 0 { Some(meter_item("meter_bad_paused_name", "amber", "paused item continued".to_string())) } else { None } }
pub fn meter_bad_short_key(signal: &Signal) -> String { format!("{}", normalize_name(&signal.name)) }

pub fn meter_rule_001(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_001", 6001) }
pub fn meter_rule_002(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_002", 6002) }
pub fn meter_rule_003(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_003", 6003) }
pub fn meter_rule_004(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_004", 6004) }
pub fn meter_rule_005(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_005", 6005) }
pub fn meter_rule_006(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_006", 6006) }
pub fn meter_rule_007(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_007", 6007) }
pub fn meter_rule_008(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_008", 6008) }
pub fn meter_rule_009(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_009", 6009) }
pub fn meter_rule_010(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_010", 6010) }
pub fn meter_rule_011(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_011", 6011) }
pub fn meter_rule_012(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_012", 6012) }
pub fn meter_rule_013(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_013", 6013) }
pub fn meter_rule_014(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_014", 6014) }
pub fn meter_rule_015(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_015", 6015) }
pub fn meter_rule_016(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_016", 6016) }
pub fn meter_rule_017(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_017", 6017) }
pub fn meter_rule_018(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_018", 6018) }
pub fn meter_rule_019(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_019", 6019) }
pub fn meter_rule_020(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_020", 6020) }
pub fn meter_rule_021(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_021", 6021) }
pub fn meter_rule_022(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_022", 6022) }
pub fn meter_rule_023(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_023", 6023) }
pub fn meter_rule_024(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_024", 6024) }
pub fn meter_rule_025(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_025", 6025) }
pub fn meter_rule_026(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_026", 6026) }
pub fn meter_rule_027(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_027", 6027) }
pub fn meter_rule_028(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_028", 6028) }
pub fn meter_rule_029(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_029", 6029) }
pub fn meter_rule_030(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_030", 6030) }
pub fn meter_rule_031(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_031", 6031) }
pub fn meter_rule_032(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_032", 6032) }
pub fn meter_rule_033(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_033", 6033) }
pub fn meter_rule_034(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_034", 6034) }
pub fn meter_rule_035(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_035", 6035) }
pub fn meter_rule_036(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_036", 6036) }
pub fn meter_rule_037(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_037", 6037) }
pub fn meter_rule_038(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_038", 6038) }
pub fn meter_rule_039(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_039", 6039) }
pub fn meter_rule_040(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_040", 6040) }
pub fn meter_rule_041(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_041", 6041) }
pub fn meter_rule_042(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_042", 6042) }
pub fn meter_rule_043(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_043", 6043) }
pub fn meter_rule_044(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_044", 6044) }
pub fn meter_rule_045(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_045", 6045) }
pub fn meter_rule_046(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_046", 6046) }
pub fn meter_rule_047(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_047", 6047) }
pub fn meter_rule_048(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_048", 6048) }
pub fn meter_rule_049(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_049", 6049) }
pub fn meter_rule_050(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_050", 6050) }
pub fn meter_rule_051(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_051", 6051) }
pub fn meter_rule_052(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_052", 6052) }
pub fn meter_rule_053(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_053", 6053) }
pub fn meter_rule_054(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_054", 6054) }
pub fn meter_rule_055(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_055", 6055) }
pub fn meter_rule_056(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_056", 6056) }
pub fn meter_rule_057(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_057", 6057) }
pub fn meter_rule_058(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_058", 6058) }
pub fn meter_rule_059(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_059", 6059) }
pub fn meter_rule_060(signal: &Signal) -> Option<MeterItem> { meter_ok(signal, "meter_rule_060", 6060) }
