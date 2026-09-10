//! Ordinary-Git synthetic fixture construction, separate from product ingestion.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

pub fn standard_git() -> Result<PathBuf, String> {
    ["/usr/bin/git", "/bin/git"].into_iter().map(PathBuf::from).find(|path| path.is_file())
        .ok_or_else(|| "Git Notes proof requires standard Git at /usr/bin/git or /bin/git; Git AI is not needed".into())
}

pub fn git(git: &Path, repo: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Output, String> {
    let mut child = Command::new(git)
        .current_dir(repo)
        .env_clear()
        .env("PATH", "")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Synthetic")
        .env("GIT_AUTHOR_EMAIL", "synthetic@example.invalid")
        .env("GIT_COMMITTER_NAME", "Synthetic")
        .env("GIT_COMMITTER_EMAIL", "synthetic@example.invalid")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .args([
            "--no-pager",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.allow=never",
        ])
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .ok_or("missing fixture stdin")?
            .write_all(input)
            .map_err(|e| e.to_string())?;
    }
    child.wait_with_output().map_err(|e| e.to_string())
}
pub fn git_ok(
    git_path: &Path,
    repo: &Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let output = git(git_path, repo, args, input)?;
    if !output.status.success() {
        return Err(format!(
            "synthetic Git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}
fn id(bytes: Vec<u8>) -> Result<String, String> {
    Ok(String::from_utf8(bytes)
        .map_err(|e| e.to_string())?
        .trim_end_matches('\n')
        .into())
}

/// Create a real repository, commit, files and note using no Git AI software.
pub fn initialize(repo: &Path, git_path: &Path, bare: bool, note: &[u8]) -> Result<String, String> {
    fs::create_dir_all(repo).map_err(|e| e.to_string())?;
    let mut args = vec!["init", "--template=", "--initial-branch=main"];
    if bare {
        args.push("--bare");
    }
    git_ok(git_path, repo, &args, None)?;
    let content = "synthetic line\n".repeat(50);
    let blob = id(git_ok(
        git_path,
        repo,
        &["hash-object", "-w", "--stdin"],
        Some(content.as_bytes()),
    )?)?;
    let tree_input = format!("100644 blob {blob}\tfile name.rs\n100644 blob {blob}\tsource.rs\n");
    let tree = id(git_ok(
        git_path,
        repo,
        &["mktree"],
        Some(tree_input.as_bytes()),
    )?)?;
    let commit = id(git_ok(
        git_path,
        repo,
        &["commit-tree", &tree],
        Some(b"Synthetic commit\n"),
    )?)?;
    git_ok(
        git_path,
        repo,
        &["update-ref", "refs/heads/main", &commit],
        None,
    )?;
    if !bare {
        git_ok(git_path, repo, &["reset", "--hard", &commit], None)?;
    }
    git_ok(
        git_path,
        repo,
        &["notes", "--ref=refs/notes/ai", "add", "-F", "-", &commit],
        Some(note),
    )?;
    Ok(commit)
}
