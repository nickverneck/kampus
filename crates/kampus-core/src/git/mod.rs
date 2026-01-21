//! Git integration for incremental indexing
//!
//! Detects changed files between commits to enable efficient incremental updates.

use git2::{DiffOptions, Repository, StatusOptions};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitError {
    #[error("Not a git repository: {0}")]
    NotARepository(String),
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),
    #[error("Invalid reference: {0}")]
    InvalidRef(String),
}

/// Result type for git operations
pub type GitResult<T> = Result<T, GitError>;

/// Type of file change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// A changed file detected by git
#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub kind: ChangeKind,
}

/// Git diff detector for finding changed files
pub struct GitDiff {
    repo: Repository,
}

impl GitDiff {
    /// Open a repository at the given path
    pub fn open(path: impl AsRef<Path>) -> GitResult<Self> {
        let repo = Repository::discover(path.as_ref()).map_err(|e| {
            if e.code() == git2::ErrorCode::NotFound {
                GitError::NotARepository(path.as_ref().display().to_string())
            } else {
                GitError::Git(e)
            }
        })?;

        Ok(Self { repo })
    }

    /// Get the current HEAD commit SHA
    pub fn head_commit(&self) -> GitResult<String> {
        let head = self.repo.head()?;
        let commit = head.peel_to_commit()?;
        Ok(commit.id().to_string())
    }

    /// Get changes between two commits
    pub fn changes_between(
        &self,
        from_commit: &str,
        to_commit: &str,
    ) -> GitResult<Vec<ChangedFile>> {
        let from_oid = self
            .repo
            .revparse_single(from_commit)?
            .peel_to_commit()?
            .id();
        let to_oid = self
            .repo
            .revparse_single(to_commit)?
            .peel_to_commit()?
            .id();

        let from_tree = self.repo.find_commit(from_oid)?.tree()?;
        let to_tree = self.repo.find_commit(to_oid)?.tree()?;

        let mut opts = DiffOptions::new();
        opts.include_untracked(false);

        let diff = self
            .repo
            .diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut opts))?;

        let mut changes = Vec::new();

        diff.foreach(
            &mut |delta, _| {
                let kind = match delta.status() {
                    git2::Delta::Added => ChangeKind::Added,
                    git2::Delta::Deleted => ChangeKind::Deleted,
                    git2::Delta::Modified => ChangeKind::Modified,
                    git2::Delta::Renamed => ChangeKind::Renamed,
                    git2::Delta::Copied => ChangeKind::Added,
                    _ => return true,
                };

                let new_path = delta.new_file().path().map(PathBuf::from);
                let old_path = delta.old_file().path().map(PathBuf::from);

                if let Some(path) = new_path.or_else(|| old_path.clone()) {
                    changes.push(ChangedFile {
                        path,
                        old_path: if kind == ChangeKind::Renamed {
                            old_path
                        } else {
                            None
                        },
                        kind,
                    });
                }

                true
            },
            None,
            None,
            None,
        )?;

        Ok(changes)
    }

    /// Get changes from a commit to the current working directory
    pub fn changes_since(&self, from_commit: &str) -> GitResult<Vec<ChangedFile>> {
        let from_oid = self
            .repo
            .revparse_single(from_commit)?
            .peel_to_commit()?
            .id();
        let from_tree = self.repo.find_commit(from_oid)?.tree()?;

        let mut opts = DiffOptions::new();
        opts.include_untracked(true);

        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(Some(&from_tree), Some(&mut opts))?;

        let mut changes = Vec::new();

        diff.foreach(
            &mut |delta, _| {
                let kind = match delta.status() {
                    git2::Delta::Added => ChangeKind::Added,
                    git2::Delta::Deleted => ChangeKind::Deleted,
                    git2::Delta::Modified => ChangeKind::Modified,
                    git2::Delta::Renamed => ChangeKind::Renamed,
                    git2::Delta::Copied => ChangeKind::Added,
                    git2::Delta::Untracked => ChangeKind::Added,
                    _ => return true,
                };

                let new_path = delta.new_file().path().map(PathBuf::from);
                let old_path = delta.old_file().path().map(PathBuf::from);

                if let Some(path) = new_path.or_else(|| old_path.clone()) {
                    changes.push(ChangedFile {
                        path,
                        old_path: if kind == ChangeKind::Renamed {
                            old_path
                        } else {
                            None
                        },
                        kind,
                    });
                }

                true
            },
            None,
            None,
            None,
        )?;

        Ok(changes)
    }

    /// Get all uncommitted changes (staged + unstaged)
    pub fn uncommitted_changes(&self) -> GitResult<Vec<ChangedFile>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);

        let statuses = self.repo.statuses(Some(&mut opts))?;
        let mut changes = Vec::new();

        for entry in statuses.iter() {
            let status = entry.status();
            let path = entry.path().map(PathBuf::from);

            if let Some(path) = path {
                let kind = if status.is_wt_new() || status.is_index_new() {
                    ChangeKind::Added
                } else if status.is_wt_deleted() || status.is_index_deleted() {
                    ChangeKind::Deleted
                } else if status.is_wt_modified() || status.is_index_modified() {
                    ChangeKind::Modified
                } else if status.is_wt_renamed() || status.is_index_renamed() {
                    ChangeKind::Renamed
                } else {
                    continue;
                };

                changes.push(ChangedFile {
                    path,
                    old_path: None,
                    kind,
                });
            }
        }

        Ok(changes)
    }

    /// Check if a path is tracked by git
    pub fn is_tracked(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        self.repo
            .status_file(path)
            .map(|status| !status.is_ignored())
            .unwrap_or(false)
    }

    /// Get the repository root path
    pub fn root(&self) -> &Path {
        self.repo.workdir().unwrap_or_else(|| self.repo.path())
    }
}

/// Filter changes to only include files with specific extensions
pub fn filter_by_extensions(
    changes: Vec<ChangedFile>,
    extensions: &HashSet<&str>,
) -> Vec<ChangedFile> {
    changes
        .into_iter()
        .filter(|change| {
            change
                .path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| extensions.contains(e))
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_kind() {
        assert_eq!(ChangeKind::Added, ChangeKind::Added);
        assert_ne!(ChangeKind::Added, ChangeKind::Modified);
    }
}
