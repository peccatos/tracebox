use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use crate::evidence::manifest::{
    GitEvidence, WorkspaceChanges, WorkspaceEvidence, WorkspaceFileState,
};

/// Best-effort git/workspace snapshot.
///
/// PR v0.1 deliberately uses the `git` CLI rather than embedding a git object
/// library. This keeps the evidence layer small and avoids coupling it to one
/// implementation. If this becomes hot or needs deeper object inspection, the
/// module can later be swapped for `gix` or a host runtime's git abstraction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub status: BTreeMap<String, FileStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Modified,
    Created,
    Deleted,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FileStatus::Modified => "modified",
            FileStatus::Created => "created",
            FileStatus::Deleted => "deleted",
        }
    }
}

impl WorkspaceSnapshot {
    pub fn dirty(&self) -> bool {
        !self.status.is_empty()
    }

    pub fn file_states(&self) -> Vec<WorkspaceFileState> {
        self.status
            .iter()
            .map(|(path, status)| WorkspaceFileState {
                path: path.clone(),
                status: status.as_str().to_string(),
            })
            .collect()
    }
}

/// Capture git metadata and dirty workspace state.
///
/// All failures degrade to partial/empty evidence. Git inspection failure must
/// not fail the traced command.
pub fn capture_workspace_snapshot(cwd: &Path) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        commit: git_output(cwd, &["rev-parse", "HEAD"]),
        branch: git_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]),
        status: git_status(cwd),
    }
}

pub fn build_git_evidence(before: &WorkspaceSnapshot, after: &WorkspaceSnapshot) -> GitEvidence {
    GitEvidence {
        commit_before: before.commit.clone(),
        commit_after: after.commit.clone(),
        branch_before: before.branch.clone(),
        branch_after: after.branch.clone(),
        dirty_before: before.dirty(),
        dirty_after: after.dirty(),
    }
}

pub fn build_workspace_evidence(
    before: &WorkspaceSnapshot,
    after: &WorkspaceSnapshot,
) -> WorkspaceEvidence {
    WorkspaceEvidence {
        dirty_before: before.file_states(),
        dirty_after: after.file_states(),
        changes: diff_snapshots(before, after),
    }
}

/// Diff before/after snapshots.
///
/// Policy:
///
/// Existing dirty files are not automatically attributed to the traced command.
/// A file is considered changed by this execution only when it newly appears,
/// disappears, or changes coarse status between snapshots.
///
/// This is conservative. It avoids false claims at the cost of missing changes
/// inside files that were already dirty before execution.
pub fn diff_snapshots(before: &WorkspaceSnapshot, after: &WorkspaceSnapshot) -> WorkspaceChanges {
    let mut all_paths = BTreeSet::new();

    for path in before.status.keys() {
        all_paths.insert(path.clone());
    }

    for path in after.status.keys() {
        all_paths.insert(path.clone());
    }

    let mut changes = WorkspaceChanges::default();

    for path in all_paths {
        let before_status = before.status.get(&path).copied();
        let after_status = after.status.get(&path).copied();

        match (before_status, after_status) {
            (None, Some(FileStatus::Created)) => changes.created_files.push(path),
            (None, Some(FileStatus::Deleted)) => changes.deleted_files.push(path),
            (None, Some(FileStatus::Modified)) => changes.modified_files.push(path),

            (Some(_), None) => {
                // Dirty before and clean after is still a workspace mutation.
                changes.modified_files.push(path);
            }

            (Some(before), Some(after)) if before != after => match after {
                FileStatus::Created => changes.created_files.push(path),
                FileStatus::Deleted => changes.deleted_files.push(path),
                FileStatus::Modified => changes.modified_files.push(path),
            },

            (Some(_), Some(_)) => {
                // Same coarse dirty state existed before and after. Without
                // content hashing, we do not attribute it to this execution.
            }

            (None, None) => {}
        }
    }

    changes
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn git_status(cwd: &Path) -> BTreeMap<String, FileStatus> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(cwd)
        .output();

    let Ok(output) = output else {
        return BTreeMap::new();
    };

    if !output.status.success() {
        return BTreeMap::new();
    }

    let Ok(text) = String::from_utf8(output.stdout) else {
        return BTreeMap::new();
    };

    let mut map = BTreeMap::new();

    for line in text.lines() {
        if line.len() < 4 {
            continue;
        }

        let status_code = &line[0..2];
        let path = parse_porcelain_path(&line[3..]);

        let status = classify_porcelain_status(status_code);
        map.insert(path, status);
    }

    map
}

fn parse_porcelain_path(raw: &str) -> String {
    // Rename lines look like:
    // R  old/path -> new/path
    //
    // For v0.1, record the destination path because that is what exists after
    // the operation. Deeper rename semantics can be added later.
    if let Some((_, new_path)) = raw.split_once(" -> ") {
        new_path.to_string()
    } else {
        raw.to_string()
    }
}

fn classify_porcelain_status(status_code: &str) -> FileStatus {
    if status_code.contains('D') {
        FileStatus::Deleted
    } else if status_code.contains('A') || status_code == "??" {
        FileStatus::Created
    } else {
        FileStatus::Modified
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{diff_snapshots, FileStatus, WorkspaceSnapshot};

    #[test]
    fn detects_created_files() {
        let before = WorkspaceSnapshot::default();

        let mut after_status = BTreeMap::new();
        after_status.insert("src/new.rs".to_string(), FileStatus::Created);

        let after = WorkspaceSnapshot {
            status: after_status,
            ..WorkspaceSnapshot::default()
        };

        let diff = diff_snapshots(&before, &after);

        assert_eq!(diff.created_files, vec!["src/new.rs"]);
        assert!(diff.modified_files.is_empty());
        assert!(diff.deleted_files.is_empty());
    }

    #[test]
    fn does_not_attribute_preexisting_dirty_file_without_status_change() {
        let mut status = BTreeMap::new();
        status.insert("src/lib.rs".to_string(), FileStatus::Modified);

        let before = WorkspaceSnapshot {
            status: status.clone(),
            ..WorkspaceSnapshot::default()
        };

        let after = WorkspaceSnapshot {
            status,
            ..WorkspaceSnapshot::default()
        };

        let diff = diff_snapshots(&before, &after);

        assert!(diff.modified_files.is_empty());
        assert!(diff.created_files.is_empty());
        assert!(diff.deleted_files.is_empty());
    }

    #[test]
    fn dirty_before_clean_after_counts_as_mutation() {
        let mut before_status = BTreeMap::new();
        before_status.insert("src/lib.rs".to_string(), FileStatus::Modified);

        let before = WorkspaceSnapshot {
            status: before_status,
            ..WorkspaceSnapshot::default()
        };

        let after = WorkspaceSnapshot::default();

        let diff = diff_snapshots(&before, &after);

        assert_eq!(diff.modified_files, vec!["src/lib.rs"]);
    }
}
