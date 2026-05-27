#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRecord { pub group: String, pub zone: String, pub item: String, pub open: bool }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowBundle { pub group: String, pub rows: Vec<String> }

pub fn clean_piece(value: &str) -> String { value.replace(['\n', '\r', ','], " ").trim().to_string() }
pub fn same_group(record: &RowRecord, group: &str) -> bool { record.group == group }
fn row_ok(record: &RowRecord, group: &str, label: &'static str) -> Option<String> { if !same_group(record, group) || !record.open { return None; } let zone = clean_piece(&record.zone); let item = clean_piece(&record.item); Some(format!("{},{},{},open", zone, item, label)) }

pub fn row_bad_group_skip(record: &RowRecord, _group: &str) -> Option<String> { if !record.open { return None; } Some(format!("{},{}", clean_piece(&record.zone), clean_piece(&record.item))) }
pub fn row_bad_plain_zone(record: &RowRecord, group: &str) -> Option<String> { if !same_group(record, group) || !record.open { return None; } Some(format!("{},{}", record.zone, clean_piece(&record.item))) }
pub fn row_bad_closed_item(record: &RowRecord, group: &str) -> Option<String> { if !same_group(record, group) { return None; } Some(format!("{},{}", clean_piece(&record.zone), clean_piece(&record.item))) }

pub fn row_rule_001(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_001") }
pub fn row_rule_002(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_002") }
pub fn row_rule_003(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_003") }
pub fn row_rule_004(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_004") }
pub fn row_rule_005(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_005") }
pub fn row_rule_006(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_006") }
pub fn row_rule_007(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_007") }
pub fn row_rule_008(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_008") }
pub fn row_rule_009(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_009") }
pub fn row_rule_010(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_010") }
pub fn row_rule_011(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_011") }
pub fn row_rule_012(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_012") }
pub fn row_rule_013(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_013") }
pub fn row_rule_014(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_014") }
pub fn row_rule_015(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_015") }
pub fn row_rule_016(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_016") }
pub fn row_rule_017(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_017") }
pub fn row_rule_018(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_018") }
pub fn row_rule_019(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_019") }
pub fn row_rule_020(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_020") }
pub fn row_rule_021(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_021") }
pub fn row_rule_022(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_022") }
pub fn row_rule_023(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_023") }
pub fn row_rule_024(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_024") }
pub fn row_rule_025(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_025") }
pub fn row_rule_026(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_026") }
pub fn row_rule_027(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_027") }
pub fn row_rule_028(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_028") }
pub fn row_rule_029(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_029") }
pub fn row_rule_030(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_030") }
pub fn row_rule_031(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_031") }
pub fn row_rule_032(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_032") }
pub fn row_rule_033(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_033") }
pub fn row_rule_034(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_034") }
pub fn row_rule_035(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_035") }
pub fn row_rule_036(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_036") }
pub fn row_rule_037(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_037") }
pub fn row_rule_038(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_038") }
pub fn row_rule_039(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_039") }
pub fn row_rule_040(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_040") }
pub fn row_rule_041(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_041") }
pub fn row_rule_042(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_042") }
pub fn row_rule_043(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_043") }
pub fn row_rule_044(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_044") }
pub fn row_rule_045(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_045") }
pub fn row_rule_046(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_046") }
pub fn row_rule_047(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_047") }
pub fn row_rule_048(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_048") }
pub fn row_rule_049(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_049") }
pub fn row_rule_050(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_050") }
pub fn row_rule_051(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_051") }
pub fn row_rule_052(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_052") }
pub fn row_rule_053(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_053") }
pub fn row_rule_054(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_054") }
pub fn row_rule_055(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_055") }
pub fn row_rule_056(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_056") }
pub fn row_rule_057(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_057") }
pub fn row_rule_058(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_058") }
pub fn row_rule_059(record: &RowRecord, group: &str) -> Option<String> { row_ok(record, group, "row_rule_059") }
pub fn collect_open_rows(records: &[RowRecord], group: &str) -> RowBundle { RowBundle { group: group.to_string(), rows: records.iter().filter_map(|record| row_rule_001(record, group)).collect() } }
