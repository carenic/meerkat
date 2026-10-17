//! A Git skill repository's HTTPS token reaches neither a command line, nor
//! an environment, nor a file: not the `git` command lines the source runs,
//! not their environments, not the checkout's `.git/config`, not a checkout
//! an older release cloned with the token in its remote URL.
//!
//! A local bare repository stands in for the HTTPS remote: `url.<base>.insteadOf`
//! rewrites send git there, and a `git` wrapper first on `PATH` logs every
//! invocation's arguments, and whether its environment holds the token,
//! before it runs git. It never logs the environment itself, which holds the
//! test process's own (the host's secrets included). Both come from this
//! process's environment, so this binary holds this one test: no sibling
//! test thread can read or race the change.

#![cfg(unix)]
#![allow(
    unsafe_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_methods
)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use meerkat_core::skills::{SkillFilter, SkillSource, SourceUuid};
use meerkat_skills::source::git::{GitRef, GitSkillAuth, GitSkillConfig, GitSkillSource};

const TOKEN: &str = "ghs-skill-token-6d1f0b73";
/// What the wrapper logs for an invocation whose environment holds the token.
const TOKEN_IN_ENVIRONMENT: &str = "environment: holds the token";

#[tokio::test]
async fn an_https_token_reaches_no_git_command_line_environment_or_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let real_git = find_on_path("git").expect("git is installed");

    // The remote, in two places: where git reaches `https://example.invalid/`
    // and where it reaches `https://x-access-token:<token>@example.invalid/`
    // (the URL an earlier release cloned).
    let work = base.join("work");
    fs::create_dir_all(work.join("skills/remote-skill")).unwrap();
    fs::write(
        work.join("skills/remote-skill/SKILL.md"),
        "---\nname: remote-skill\ndescription: From the remote\n---\n\n# Remote",
    )
    .unwrap();
    let plain = base.join("plain");
    let tokenized = base.join("tokenized");
    publish(&real_git, &work, &plain.join("skills.git"));
    publish(
        &real_git,
        &work,
        &tokenized.join(format!("{TOKEN}@example.invalid/skills.git")),
    );

    let log = base.join("git-invocations.log");
    let bin = base.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n\
             {{ printf 'argv:'; for argument in \"$@\"; do printf ' %s' \"$argument\"; done; \
             printf '\\n'; }} >> '{log}'\n\
             if env | grep -q -F '{TOKEN}'; then echo '{TOKEN_IN_ENVIRONMENT}' >> '{log}'; fi\n\
             exec '{git}' \"$@\"\n",
            log = log.display(),
            git = real_git.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();

    let mut path = vec![bin.clone()];
    path.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    // SAFETY: this binary runs this one test; nothing else reads or writes
    // the process environment while it changes.
    unsafe {
        std::env::set_var("PATH", std::env::join_paths(path).unwrap());
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_CONFIG_GLOBAL", base.join("no-global-config"));
        std::env::set_var("GIT_CONFIG_COUNT", "2");
        std::env::set_var(
            "GIT_CONFIG_KEY_0",
            format!("url.file://{}/.insteadOf", plain.display()),
        );
        std::env::set_var("GIT_CONFIG_VALUE_0", "https://example.invalid/");
        std::env::set_var(
            "GIT_CONFIG_KEY_1",
            format!("url.file://{}/.insteadOf", tokenized.display()),
        );
        std::env::set_var("GIT_CONFIG_VALUE_1", "https://x-access-token:");
    }

    let cache = base.join("cache");
    let source = GitSkillSource::new(GitSkillConfig {
        repo_url: "https://example.invalid/skills.git".to_string(),
        git_ref: GitRef::Branch("main".to_string()),
        cache_dir: cache.clone(),
        skills_root: Some("skills".to_string()),
        // Every listing refreshes: the second one fetches into the checkout.
        refresh_interval: Duration::ZERO,
        auth: Some(GitSkillAuth::HttpsToken(TOKEN.to_string())),
        depth: None,
        source_uuid: SourceUuid::parse("a8f3c2d1-4b5e-4f60-8a71-92b3c4d5e6f7").unwrap(),
        health_thresholds: Default::default(),
    });

    let mut leaks = Vec::new();
    let listed = source.list(&SkillFilter::default()).await;
    assert!(
        listed.as_ref().is_ok_and(|skills| skills
            .iter()
            .any(|s| s.key.skill_name.as_str() == "remote-skill")),
        "the clone loads the remote's skill: {listed:?}"
    );
    leaks.extend(files_holding_the_token(&cache));

    // A checkout an earlier release cloned holds the token in its remote
    // URL; the next refresh takes it out.
    let checkout = checkout_dir(&cache);
    git(
        &real_git,
        &checkout,
        &[
            "config",
            "remote.origin.url",
            &format!("https://x-access-token:{TOKEN}@example.invalid/skills.git"),
        ],
    );
    let listed = source.list(&SkillFilter::default()).await;
    assert!(
        listed.as_ref().is_ok_and(|skills| skills
            .iter()
            .any(|s| s.key.skill_name.as_str() == "remote-skill")),
        "the fetch keeps the remote's skill: {listed:?}"
    );
    leaks.extend(
        files_holding_the_token(&cache)
            .into_iter()
            .map(|file| format!("{file} (after refreshing an older checkout)")),
    );

    let invocations = fs::read_to_string(&log).unwrap();
    let command_lines: Vec<&str> = invocations
        .lines()
        .filter(|line| line.starts_with("argv:"))
        .collect();
    assert!(
        command_lines
            .iter()
            .any(|line| line.split(' ').any(|word| word == "clone")),
        "the wrapper saw the clone:\n{}",
        command_lines.join("\n")
    );
    for line in invocations
        .lines()
        .filter(|line| line.contains(TOKEN) || *line == TOKEN_IN_ENVIRONMENT)
    {
        leaks.push(format!("git invocation: {line}"));
    }
    assert!(
        leaks.is_empty(),
        "the token reached a git command line, environment or file:\n{}",
        leaks.join("\n")
    );
}

/// Commit `work` and push it to a new bare repository at `remote`.
fn publish(git_program: &Path, work: &Path, remote: &Path) {
    fs::create_dir_all(remote).unwrap();
    git(
        git_program,
        remote,
        &["init", "--bare", "--initial-branch=main"],
    );
    if !work.join(".git").is_dir() {
        git(git_program, work, &["init", "--initial-branch=main"]);
        git(
            git_program,
            work,
            &["config", "user.email", "skills@example.com"],
        );
        git(git_program, work, &["config", "user.name", "Skills Test"]);
        git(git_program, work, &["add", "."]);
        git(git_program, work, &["commit", "-m", "initial"]);
    }
    git(
        git_program,
        work,
        &["push", remote.to_string_lossy().as_ref(), "main"],
    );
}

fn git(git_program: &Path, cwd: &Path, args: &[&str]) {
    let status = Command::new(git_program)
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

/// The one checkout under the skill cache.
fn checkout_dir(cache: &Path) -> PathBuf {
    let mut checkouts: Vec<PathBuf> = fs::read_dir(cache)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join(".git").is_dir())
        .collect();
    assert_eq!(checkouts.len(), 1, "one checkout: {checkouts:?}");
    checkouts.remove(0)
}

/// Every file under `root` whose bytes contain the token.
fn files_holding_the_token(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && fs::read(&path)
                    .unwrap_or_default()
                    .windows(TOKEN.len())
                    .any(|window| window == TOKEN.as_bytes())
            {
                found.push(path.display().to_string());
            }
        }
    }
    found
}
