//! The workflows' path filters, read the way GitHub reads them: a change
//! that a test or a build depends on must start the workflow that checks
//! it. Docs-only changes skip `ci`, but some Markdown is a test's input, and
//! a docs edit is the most likely first contribution (code health
//! B-PERIPHERY-2).

use regex::Regex;
use std::path::Path;

/// A workflow's `paths` or `paths-ignore` list for one event.
#[derive(Debug, PartialEq)]
struct Filter {
    ignore: bool,
    patterns: Vec<String>,
}

/// The filter under `on.<event>`, from the YAML's own layout: two spaces
/// for the event, four for the list's name, six for its items.
fn filter(workflow: &str, event: &str) -> Option<Filter> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows").join(workflow)).unwrap();
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r')).skip_while(|l| *l != "on:").skip(1);
    let header = format!("  {event}:");
    lines.by_ref().take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with('#')).find(|l| *l == header)?;
    let mut filter: Option<Filter> = None;
    for line in lines {
        if !line.starts_with("    ") && !line.trim_start().starts_with('#') {
            break;
        }
        match line.trim() {
            "paths:" => filter = Some(Filter { ignore: false, patterns: vec![] }),
            "paths-ignore:" => filter = Some(Filter { ignore: true, patterns: vec![] }),
            item if line.starts_with("      - ") => {
                if let Some(f) = filter.as_mut() {
                    f.patterns.push(item.trim_start_matches("- ").trim_matches('\'').to_string());
                }
            }
            _ => {}
        }
    }
    filter
}

/// GitHub's glob: `**` crosses folders, `*` and `?` don't.
fn glob(pattern: &str) -> Regex {
    let mut re = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                re.push_str(".*");
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    Regex::new(&re).unwrap()
}

/// Whether the last pattern that matches `path` includes it (a `!` pattern
/// excludes it); None when none matches.
fn last_match(patterns: &[String], path: &str) -> Option<bool> {
    patterns.iter().rev().find_map(|p| match p.strip_prefix('!') {
        Some(neg) => glob(neg).is_match(path).then_some(false),
        None => glob(p).is_match(path).then_some(true),
    })
}

/// Whether a push that changes `changed` starts `workflow`. Its pull
/// requests must filter the same way.
fn runs(workflow: &str, changed: &[&str]) -> bool {
    let push = filter(workflow, "push").unwrap_or_else(|| panic!("{workflow} has no path filter on push"));
    assert_eq!(filter(workflow, "pull_request").as_ref(), Some(&push), "{workflow}: push and pull_request filter differently");
    if push.ignore {
        !changed.iter().all(|path| last_match(&push.patterns, path) == Some(true))
    } else {
        changed.iter().any(|path| last_match(&push.patterns, path) == Some(true))
    }
}

#[test]
fn the_glob_reads_as_github_does() {
    assert!(glob("**.md").is_match("docs/dev/RELEASE.md") && glob("**.md").is_match("README.md"));
    assert!(glob("src/**").is_match("src/server/ui.rs"));
    assert!(glob("LICENSE*").is_match("LICENSE-MIT") && !glob("LICENSE*").is_match("docs/LICENSE-MIT"));
    assert!(!glob("docs/API.md").is_match("docs/APIxmd"));
}

#[test]
fn ci_runs_for_the_markdown_tests_read() {
    // Read by tests: `every_path_is_in_api_md` (docs/API.md and its pages),
    // the verdicts service's contract test (docs/VERDICTS.md only) and the
    // price parsers' fixtures. docs/verdicts/ isn't read by a test, but its
    // pages are the rest of that contract, so CI runs for them too.
    for read in
        ["docs/API.md", "docs/api/picks.md", "docs/VERDICTS.md", "docs/verdicts/snapshot.md", "tests/fixtures/prices/openai_pricing.md"]
    {
        assert!(runs("ci.yml", &[read]), "{read}");
    }
    for code in [
        "src/main.rs",
        "build.rs",
        "web/src/App.tsx",
        "services/verdicts/src/api.ts",
        "contrib/linux/smoke-test.sh",
        ".github/workflows/ci.yml",
    ] {
        assert!(runs("ci.yml", &[code]), "{code}");
    }
    for docs in [
        "README.md",
        "CHANGELOG.md",
        "docs/stores.md",
        "docs/dev/RELEASE.md",
        "LICENSE-MIT",
        "android/app/build.gradle.kts",
        ".github/workflows/android.yml",
        "site/index.html",
        "site/styles.css",
    ] {
        assert!(!runs("ci.yml", &[docs]), "{docs}");
    }
    assert!(runs("ci.yml", &["README.md", "src/main.rs"]));
}

#[test]
fn windows_and_docker_run_for_build_rs() {
    // build.rs's Windows resources compile only on Windows, and the image
    // builds with it.
    assert!(runs("windows.yml", &["build.rs"]));
    assert!(runs("docker.yml", &["build.rs"]));
}

/// Each job of a workflow with its lines, from the YAML's own layout: two
/// spaces for a job's name under `jobs:`.
fn jobs(workflow: &str) -> Vec<(String, Vec<String>)> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows").join(workflow)).unwrap();
    let mut out: Vec<(String, Vec<String>)> = vec![];
    for line in text.lines().map(|l| l.trim_end_matches('\r')).skip_while(|l| *l != "jobs:").skip(1) {
        let name = line.strip_prefix("  ").filter(|n| !n.starts_with([' ', '#'])).and_then(|n| n.strip_suffix(':'));
        match name {
            Some(name) => out.push((name.to_string(), vec![])),
            None => {
                if let Some((_, lines)) = out.last_mut() {
                    lines.push(line.trim().to_string());
                }
            }
        }
    }
    out
}

#[test]
fn release_plans_pull_requests_read_only_and_only_for_its_configuration() {
    // A pull request starts it only when it changes what dist plans from
    // (code health B-PERIPHERY-7).
    let pr = filter("release.yml", "pull_request").expect("release.yml has no path filter on pull requests");
    assert!(!pr.ignore);
    for planned in ["Cargo.toml", "Cargo.lock", "dist-workspace.toml", ".github/build-setup.yml", ".github/workflows/release.yml"] {
        assert_eq!(last_match(&pr.patterns, planned), Some(true), "{planned}");
    }
    for other in ["src/main.rs", "web/package.json", "README.md", ".github/workflows/ci.yml"] {
        assert_ne!(last_match(&pr.patterns, other), Some(true), "{other}");
    }

    // Then only plan-pr runs, with a read-only token; the job that holds the
    // write token runs for a tag alone, and every other job waits on
    // another, so none starts on its own.
    let jobs = jobs("release.yml");
    let job = |name: &str| &jobs.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no job {name}")).1;
    let has = |lines: &Vec<String>, line: &str| lines.iter().any(|l| l == line);
    let plan_pr = job("plan-pr");
    assert!(has(plan_pr, "if: ${{ github.event_name == 'pull_request' }}") && has(plan_pr, "contents: read"), "{plan_pr:?}");
    assert!(!has(plan_pr, "contents: write") && !plan_pr.iter().any(|l| l.contains("dist host")), "{plan_pr:?}");
    let plan = job("plan");
    assert!(has(plan, "if: ${{ github.event_name != 'pull_request' }}") && has(plan, "contents: write"), "{plan:?}");
    for (name, lines) in &jobs {
        if name != "plan" && name != "plan-pr" {
            assert!(has(lines, "needs:"), "{name} waits on no other job, so it would run for a pull request");
        }
    }
}
