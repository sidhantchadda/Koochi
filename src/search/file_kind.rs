#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    All,
    Source,
    Tests,
    Configs,
}

pub fn kind_matches(path: &str, kind: FileKind) -> bool {
    match kind {
        FileKind::All => true,
        FileKind::Source => is_source(path),
        FileKind::Tests => is_test(path),
        FileKind::Configs => is_config(path),
    }
}

fn is_source(path: &str) -> bool {
    matches!(
        extension(path),
        Some(
            "rs" | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "py"
                | "go"
                | "java"
                | "kt"
                | "kts"
                | "c"
                | "cc"
                | "cpp"
                | "h"
                | "hpp"
                | "cs"
                | "rb"
                | "php"
                | "swift"
        )
    )
}

fn is_test(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("_test.")
        || lower.contains(".test.")
        || lower.contains("_spec.")
        || lower.contains(".spec.")
}

fn is_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "go.mod"
            | "pom.xml"
            | "build.gradle"
            | "koochi.toml"
            | "dockerfile"
    ) || matches!(
        extension(path),
        Some("toml" | "json" | "yaml" | "yml" | "xml" | "ini")
    )
}

fn extension(path: &str) -> Option<&str> {
    path.rsplit_once('.').map(|(_, extension)| extension)
}
