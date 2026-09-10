//! Read-only, pinned Git objects through an explicitly injected standard Git executable.
//! No Git AI installation, code, cache, helper or network protocol is used.
#![deny(unsafe_op_in_unsafe_fn)]

use std::path::PathBuf;
use unisphere_core::{
    GitNoteLoader, GitNoteRef, GitNotesError, GitNotesLimits, GitNotesListing, GitNotesScope,
    LoadedGitNote,
};

#[derive(Debug, Clone)]
pub struct GitObjectLoader {
    executable: PathBuf,
}
impl GitObjectLoader {
    /// Construction performs no I/O. Execution requires an absolute executable path.
    pub fn new(executable: PathBuf) -> Self {
        Self { executable }
    }
}
impl GitNoteLoader for GitObjectLoader {
    fn list_notes(
        &self,
        scope: &GitNotesScope,
        limits: GitNotesLimits,
    ) -> Result<GitNotesListing, GitNotesError> {
        scope.validate(limits)?;
        #[cfg(unix)]
        {
            unix::list(self, scope, limits)
        }
        #[cfg(not(unix))]
        {
            Err(GitNotesError::UnsupportedPlatform)
        }
    }
    fn read_note(
        &self,
        source: &GitNoteRef,
        limits: GitNotesLimits,
    ) -> Result<LoadedGitNote, GitNotesError> {
        source.validate()?;
        limits.validate()?;
        #[cfg(unix)]
        {
            unix::read(self, source, limits)
        }
        #[cfg(not(unix))]
        {
            Err(GitNotesError::UnsupportedPlatform)
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        collections::BTreeMap,
        fs,
        io::{self, Read},
        os::unix::process::CommandExt,
        path::Path,
        process::{Child, Command, ExitStatus, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    use unisphere_core::{GitNoteSelection, valid_object_id};

    struct Output {
        status: ExitStatus,
        bytes: Vec<u8>,
        stderr: Vec<u8>,
    }
    struct Repository {
        path: PathBuf,
        common: PathBuf,
        git_dir: PathBuf,
        worktree: Option<PathBuf>,
    }
    struct Entry {
        mode: String,
        kind: String,
        oid: String,
        path: String,
    }

    fn capture_pipe(mut pipe: impl Read, limit: usize) -> Result<Vec<u8>, GitNotesError> {
        let mut bytes = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            let count = pipe
                .read(&mut buffer[..(limit.saturating_sub(bytes.len()) + 1).min(8192)])
                .map_err(|_| GitNotesError::ObjectRead)?;
            if count == 0 {
                return Ok(bytes);
            }
            if count > limit.saturating_sub(bytes.len()) {
                return Err(GitNotesError::ListingLimit);
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
    }
    fn terminate(child: &mut Child) {
        // SAFETY: Command::process_group(0) created this owned child's process group.
        // Killing the group also closes inherited pipes if a failing executable left a child.
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    fn command(
        loader: &GitObjectLoader,
        repo: &Path,
        args: &[&str],
        limit: usize,
        limits: GitNotesLimits,
    ) -> Result<Output, GitNotesError> {
        if !loader.executable.is_absolute() {
            return Err(GitNotesError::GitUnavailable);
        }
        let mut child = Command::new(&loader.executable)
            .current_dir(repo)
            .env_clear()
            .env("PATH", "")
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args([
                "--no-pager",
                "--literal-pathspecs",
                "--no-replace-objects",
                "--no-optional-locks",
                "-c",
                "protocol.allow=never",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                    GitNotesError::GitUnavailable
                }
                _ => GitNotesError::ObjectRead,
            })?;
        let stdout = child.stdout.take().ok_or(GitNotesError::ObjectRead)?;
        let stderr = child.stderr.take().ok_or(GitNotesError::ObjectRead)?;
        let output = thread::scope(|threads| {
            let (tx, rx) = mpsc::channel();
            let out_tx = tx.clone();
            threads.spawn(move || {
                let _ = out_tx.send((0, capture_pipe(stdout, limit)));
            });
            threads.spawn(move || {
                let _ = tx.send((1, capture_pipe(stderr, 65_536)));
            });
            let mut pipes: [Option<Vec<u8>>; 2] = [None, None];
            let mut status = None;
            let start = Instant::now();
            loop {
                match rx.recv_timeout(Duration::from_millis(2)) {
                    Ok((index, Ok(bytes))) => pipes[index] = Some(bytes),
                    Ok((_, Err(error))) => {
                        terminate(&mut child);
                        return Err(error);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        thread::sleep(Duration::from_millis(2))
                    }
                }
                if status.is_none() {
                    status = match child.try_wait() {
                        Ok(status) => status,
                        Err(_) => {
                            terminate(&mut child);
                            return Err(GitNotesError::ObjectRead);
                        }
                    };
                }
                if let Some(status) = status
                    && pipes.iter().all(Option::is_some)
                {
                    return Ok(Output {
                        status,
                        bytes: pipes[0].take().unwrap(),
                        stderr: pipes[1].take().unwrap(),
                    });
                }
                if start.elapsed() >= Duration::from_millis(limits.command_timeout_ms) {
                    terminate(&mut child);
                    return Err(GitNotesError::Timeout);
                }
            }
        })?;
        if !output.status.success()
            && output
                .stderr
                .starts_with(b"fatal: detected dubious ownership in repository")
        {
            return Err(GitNotesError::UnsafeRepository);
        }
        Ok(output)
    }
    fn success(
        loader: &GitObjectLoader,
        repo: &Path,
        args: &[&str],
        limit: usize,
        limits: GitNotesLimits,
    ) -> Result<Vec<u8>, GitNotesError> {
        let output = command(loader, repo, args, limit, limits)?;
        if !output.status.success() {
            return Err(GitNotesError::ObjectRead);
        }
        Ok(output.bytes)
    }
    fn line(bytes: &[u8]) -> Result<&str, GitNotesError> {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| s.strip_suffix('\n'))
            .ok_or(GitNotesError::ObjectRead)
    }
    fn canonical(path: impl AsRef<Path>) -> Result<PathBuf, GitNotesError> {
        let path = fs::canonicalize(path).map_err(|_| GitNotesError::ObjectRead)?;
        if path.to_str().is_none() {
            return Err(GitNotesError::InvalidInput);
        }
        Ok(path)
    }
    fn open(
        loader: &GitObjectLoader,
        input: &Path,
        limits: GitNotesLimits,
    ) -> Result<Repository, GitNotesError> {
        let path = canonical(input)?;
        if !path.is_dir() {
            return Err(GitNotesError::ObjectRead);
        }
        // Effective config includes config.worktree when enabled. Global/system/inherited config is disabled.
        let config = command(
            loader,
            &path,
            &[
                "config",
                "--get-regexp",
                "^(extensions\\.partialclone|remote\\..*\\.promisor)$",
            ],
            65_536,
            limits,
        )?;
        match config.status.code() {
            Some(0) => return Err(GitNotesError::UnsupportedRepository),
            Some(1) if config.bytes.is_empty() && config.stderr.is_empty() => {}
            _ => return Err(GitNotesError::ObjectRead),
        }
        let common = canonical(line(&success(
            loader,
            &path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            65_536,
            limits,
        )?)?)?;
        let git_dir = canonical(line(&success(
            loader,
            &path,
            &["rev-parse", "--absolute-git-dir"],
            65_536,
            limits,
        )?)?)?;
        let bare = success(
            loader,
            &path,
            &["rev-parse", "--is-bare-repository"],
            16,
            limits,
        )?;
        let worktree = match line(&bare)? {
            "true" => None,
            "false" => Some(canonical(line(&success(
                loader,
                &path,
                &["rev-parse", "--show-toplevel"],
                65_536,
                limits,
            )?)?)?),
            _ => return Err(GitNotesError::ObjectRead),
        };
        Ok(Repository {
            path,
            common,
            git_dir,
            worktree,
        })
    }
    fn object_type(
        loader: &GitObjectLoader,
        repo: &Path,
        oid: &str,
        limits: GitNotesLimits,
    ) -> Result<String, GitNotesError> {
        if !valid_object_id(oid) {
            return Err(GitNotesError::InvalidData);
        }
        Ok(line(&success(
            loader,
            repo,
            &["cat-file", "-t", oid],
            32,
            limits,
        )?)?
        .into())
    }
    fn entries(bytes: &[u8]) -> Result<Vec<Entry>, GitNotesError> {
        if !bytes.is_empty() && bytes.last() != Some(&0) {
            return Err(GitNotesError::InvalidData);
        }
        bytes
            .split(|b| *b == 0)
            .filter(|row| !row.is_empty())
            .map(|row| {
                let text = std::str::from_utf8(row).map_err(|_| GitNotesError::InvalidData)?;
                let (header, path) = text.split_once('\t').ok_or(GitNotesError::InvalidData)?;
                let mut words = header.split(' ');
                let mode = words.next().ok_or(GitNotesError::InvalidData)?;
                let kind = words.next().ok_or(GitNotesError::InvalidData)?;
                let oid = words.next().ok_or(GitNotesError::InvalidData)?;
                if words.next().is_some() || !valid_object_id(oid) {
                    return Err(GitNotesError::InvalidData);
                }
                Ok(Entry {
                    mode: mode.into(),
                    kind: kind.into(),
                    oid: oid.into(),
                    path: path.into(),
                })
            })
            .collect()
    }
    fn blob(entry: &Entry) -> Result<(), GitNotesError> {
        if entry.kind != "blob" || !matches!(entry.mode.as_str(), "100644" | "100755") {
            return Err(GitNotesError::InvalidData);
        }
        Ok(())
    }
    fn selected(
        loader: &GitObjectLoader,
        repo: &Path,
        tip: &str,
        target: &str,
        budget: &mut usize,
        limits: GitNotesLimits,
    ) -> Result<Option<String>, GitNotesError> {
        let mut tree = tip.to_owned();
        let mut suffix = target;
        let mut found = None;
        loop {
            let prefix = &suffix[..2];
            let bytes = success(
                loader,
                repo,
                &["ls-tree", "--full-tree", "-z", &tree, "--", suffix, prefix],
                *budget,
                limits,
            )?;
            *budget = budget
                .checked_sub(bytes.len())
                .ok_or(GitNotesError::ListingLimit)?;
            let mut next = None;
            for entry in entries(&bytes)? {
                if entry.path == suffix {
                    blob(&entry)?;
                    if found.replace(entry.oid).is_some() {
                        return Err(GitNotesError::InvalidData);
                    }
                } else if suffix.len() > 2
                    && entry.path == prefix
                    && entry.kind == "tree"
                    && entry.mode == "040000"
                {
                    if next.replace(entry.oid).is_some() {
                        return Err(GitNotesError::InvalidData);
                    }
                } else {
                    return Err(GitNotesError::InvalidData);
                }
            }
            match next {
                Some(oid) => {
                    tree = oid;
                    suffix = &suffix[2..];
                }
                None => return Ok(found),
            }
        }
    }
    pub(super) fn list(
        loader: &GitObjectLoader,
        scope: &GitNotesScope,
        limits: GitNotesLimits,
    ) -> Result<GitNotesListing, GitNotesError> {
        let repo = open(loader, &scope.repository, limits)?;
        let ref_check = command(
            loader,
            &repo.path,
            &["check-ref-format", &scope.notes_ref],
            4096,
            limits,
        )?;
        if !ref_check.status.success() {
            return Err(GitNotesError::InvalidRef);
        }
        let tip = command(
            loader,
            &repo.path,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &scope.notes_ref,
            ],
            256,
            limits,
        )?;
        let notes_tip = match tip.status.code() {
            Some(0) => {
                let id = line(&tip.bytes)?.to_owned();
                if !valid_object_id(&id) {
                    return Err(GitNotesError::InvalidRef);
                }
                if object_type(loader, &repo.path, &id, limits)? != "commit" {
                    return Err(GitNotesError::InvalidRef);
                }
                Some(id)
            }
            Some(1) if tip.bytes.is_empty() && tip.stderr.is_empty() => None,
            _ => return Err(GitNotesError::ObjectRead),
        };
        let selection = scope.selection.normalized();
        let mut listing = GitNotesListing {
            repository: repo.path,
            repository_id: repo.common,
            git_dir: repo.git_dir,
            worktree_root: repo.worktree,
            notes_ref: scope.notes_ref.clone(),
            notes_tip,
            selection,
            notes: Vec::new(),
        };
        let Some(tip) = &listing.notes_tip else {
            return Ok(listing);
        };
        let mut candidates = BTreeMap::new();
        match &listing.selection {
            GitNoteSelection::All => {
                let bytes = success(
                    loader,
                    &listing.repository,
                    &["ls-tree", "-r", "-z", "--full-tree", tip],
                    limits.max_listing_bytes,
                    limits,
                )?;
                for entry in entries(&bytes)? {
                    blob(&entry)?;
                    let pieces: Vec<_> = entry.path.split('/').collect();
                    let target = pieces.concat();
                    if !valid_object_id(&target)
                        || target.len() != tip.len()
                        || pieces[..pieces.len() - 1]
                            .iter()
                            .any(|part| part.len() != 2)
                        || candidates.insert(target, entry.oid).is_some()
                    {
                        return Err(GitNotesError::InvalidData);
                    }
                    if candidates.len() > limits.max_notes {
                        return Err(GitNotesError::ListingLimit);
                    }
                }
            }
            GitNoteSelection::Commits(ids) => {
                let mut budget = limits.max_listing_bytes;
                for id in ids {
                    if let Some(blob) =
                        selected(loader, &listing.repository, tip, id, &mut budget, limits)?
                    {
                        candidates.insert(id.clone(), blob);
                    }
                }
            }
        }
        for (target, blob) in candidates {
            if object_type(loader, &listing.repository, &target, limits)? != "commit" {
                return Err(GitNotesError::UnsupportedTarget);
            }
            listing.notes.push(GitNoteRef {
                repository: listing.repository.clone(),
                repository_id: listing.repository_id.clone(),
                notes_ref: listing.notes_ref.clone(),
                notes_tip: tip.clone(),
                target_commit: target,
                note_blob: blob,
            });
        }
        listing.validate(scope, limits)?;
        Ok(listing)
    }
    pub(super) fn read(
        loader: &GitObjectLoader,
        source: &GitNoteRef,
        limits: GitNotesLimits,
    ) -> Result<LoadedGitNote, GitNotesError> {
        let repo = open(loader, &source.repository, limits)?;
        if repo.path != source.repository || repo.common != source.repository_id {
            return Err(GitNotesError::InvalidData);
        }
        if object_type(loader, &repo.path, &source.notes_tip, limits)? != "commit" {
            return Err(GitNotesError::InvalidRef);
        }
        let mut budget = limits.max_listing_bytes;
        if selected(
            loader,
            &repo.path,
            &source.notes_tip,
            &source.target_commit,
            &mut budget,
            limits,
        )?
        .as_deref()
            != Some(source.note_blob.as_str())
        {
            return Err(GitNotesError::InvalidData);
        }
        if object_type(loader, &repo.path, &source.target_commit, limits)? != "commit" {
            return Err(GitNotesError::UnsupportedTarget);
        }
        let size_bytes = success(
            loader,
            &repo.path,
            &["cat-file", "-s", &source.note_blob],
            64,
            limits,
        )?;
        let size: usize = line(&size_bytes)?
            .parse()
            .map_err(|_| GitNotesError::ObjectRead)?;
        if size > limits.max_note_bytes {
            return Err(GitNotesError::NoteLimit);
        }
        let bytes = success(
            loader,
            &repo.path,
            &["cat-file", "blob", &source.note_blob],
            limits.max_note_bytes,
            limits,
        )
        .map_err(|error| {
            if error == GitNotesError::ListingLimit {
                GitNotesError::NoteLimit
            } else {
                error
            }
        })?;
        if bytes.len() != size {
            return Err(GitNotesError::ObjectRead);
        }
        Ok(LoadedGitNote {
            source: source.clone(),
            bytes,
        })
    }
}
