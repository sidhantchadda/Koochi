use crate::support::copy_fixture_codebase;
use std::fs;
use std::path::Path;
use std::process::Command;

const BASELINE_SOURCE_LINES: usize = 3_500;
const REVIEW_COMMIT_ADDED_LINES: usize = 2_000;

pub fn create_claims_review_repo(repo: &Path) {
    copy_fixture_codebase("rust", "consistency", repo);
    rebuild_inner_git_history(repo);
}

fn rebuild_inner_git_history(repo: &Path) {
    let git_dir = repo.join(".git");
    if git_dir.exists() {
        fs::remove_dir_all(&git_dir).unwrap();
    }

    let final_lib = fs::read_to_string(repo.join("src").join("lib.rs")).unwrap();
    let review_temp = tempfile::tempdir().unwrap();
    fs::rename(
        repo.join("src").join("claims"),
        review_temp.path().join("claims"),
    )
    .unwrap();
    fs::write(repo.join("src").join("lib.rs"), "pub mod baseline;\n").unwrap();

    assert_eq!(
        source_line_count(&repo.join("src")),
        BASELINE_SOURCE_LINES,
        "claims review fixture should stay in the requested 3-4k LOC baseline range"
    );

    git_init(repo);
    git(
        repo,
        &["add", ".gitignore", "Cargo.toml", "koochi.toml", "src"],
    );
    git(repo, &["commit", "-m", "baseline claims fixture"]);

    fs::rename(
        review_temp.path().join("claims"),
        repo.join("src").join("claims"),
    )
    .unwrap();
    fs::write(repo.join("src").join("lib.rs"), final_lib).unwrap();
    git(repo, &["add", "src"]);
    git(repo, &["commit", "-m", "add claims review surface"]);

    assert_eq!(
        head_commit_added_lines(repo),
        REVIEW_COMMIT_ADDED_LINES,
        "review commit should add exactly {REVIEW_COMMIT_ADDED_LINES} LOC"
    );
    assert_eq!(
        git_stdout(repo, &["rev-list", "--count", "HEAD"]).trim(),
        "2",
        "claims review fixture should have baseline and review commits"
    );
    assert_eq!(
        git_stdout(repo, &["status", "--short"]).trim(),
        "",
        "claims review fixture should be clean before Koochi runs"
    );
}

fn source_line_count(path: &Path) -> usize {
    let mut total = 0;
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            total += source_line_count(&path);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            total += fs::read_to_string(path).unwrap().lines().count();
        }
    }
    total
}

fn head_commit_added_lines(repo: &Path) -> usize {
    git_stdout(repo, &["show", "--numstat", "--format=", "HEAD"])
        .lines()
        .filter_map(|line| line.split('\t').next())
        .map(|value| value.parse::<usize>().unwrap())
        .sum()
}

fn git_init(repo: &Path) {
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "koochi@example.test"]);
    git(repo, &["config", "user.name", "Koochi"]);
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
