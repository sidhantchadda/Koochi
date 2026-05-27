#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceId(pub String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemId(pub String);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Units(pub i64);
pub fn join_key(workspace: &WorkspaceId, item: &ItemId) -> String { format!("{}:{}", workspace.0, item.0) }
pub fn bounded_units(value: i64) -> Units { Units(value.clamp(0, 1_000_000)) }
