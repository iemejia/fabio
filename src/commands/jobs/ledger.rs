//! Serialized, crash-safe local receipt snapshots. The stable sidecar lock must
//! never be renamed or deleted: locking the replaced data inode loses exclusion.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;

use super::JobEntry;
use crate::errors::{ErrorCode, FabioError, HintType};

const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const RECOVERY_HINT: &str = "Preserve jobs.jsonl and any .jobs-*.tmp files privately for investigation. Restore a verified private backup only after all fabio processes exit; never delete the lock file or replay a remote submission to recreate its receipt. A failed write after publication may already have committed.";

pub(super) struct Ledger {
    path: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommitStage {
    Staged,
    Published,
}

impl Ledger {
    pub(super) const fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(super) fn append(&self, entry: &JobEntry) -> Result<()> {
        self.edit(|entries| {
            if entries.iter().any(|existing| existing.id == entry.id) {
                return Err(FabioError::with_typed_hint(
                    ErrorCode::Conflict,
                    "A receipt with this job ID already exists; the ledger was not changed.",
                    "Inspect the existing receipt with jobs get. Do not replay a remote submission to obtain another ID.",
                    HintType::SemanticCorrection,
                ).set_retriable(Some(false)).into());
            }
            entries.push(entry.clone());
            Ok(())
        })
    }

    pub(super) fn read_all(&self) -> Result<Vec<JobEntry>> {
        let _lock = self.lock(false, LOCK_TIMEOUT)?;
        self.read_unlocked()
    }

    pub(super) fn edit<T>(&self, edit: impl FnOnce(&mut Vec<JobEntry>) -> Result<T>) -> Result<T> {
        let _lock = self.lock(true, LOCK_TIMEOUT)?;
        let mut entries = self.read_unlocked()?;
        let value = edit(&mut entries)?;
        let mut ids = HashSet::new();
        for (index, entry) in entries.iter().enumerate() {
            if entry.id.trim().is_empty() || !ids.insert(&entry.id) {
                return Err(invalid_record(index + 1, "empty or duplicate job ID"));
            }
        }
        self.write_atomic(&entries, |_, _| Ok(()))?;
        Ok(value)
    }

    fn parent(&self) -> &Path {
        self.path.parent().expect("ledger has a parent directory")
    }

    fn lock(&self, exclusive: bool, timeout: Duration) -> Result<File> {
        let parent = self.parent();
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory
            .create(parent)
            .map_err(|error| storage_error("create its directory", &error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| storage_error("restrict its directory", &error))?;
            // Persist creation of .fabio itself before publishing a first receipt.
            if let Some(ancestor) = parent.parent() {
                sync_directory(ancestor)?;
            }
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(self.path.with_extension("lock"))
            .map_err(|error| storage_error("open its stable lock", &error))?;
        let start = Instant::now();
        loop {
            let result = if exclusive {
                file.try_lock()
            } else {
                file.try_lock_shared()
            };
            match result {
                Ok(()) => return Ok(file),
                Err(TryLockError::Error(error)) => return Err(storage_error("lock", &error)),
                Err(TryLockError::WouldBlock) if start.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(TryLockError::WouldBlock) => return Err(FabioError::with_typed_hint(
                    ErrorCode::Conflict,
                    "The local receipt ledger is busy; the lock wait expired.",
                    "Wait for the other fabio process to finish, then inspect jobs list/get. Never delete jobs.lock or resubmit a remote operation because local bookkeeping was busy.",
                    HintType::SemanticCorrection,
                ).set_retriable(Some(false)).into()),
            }
        }
    }

    fn read_unlocked(&self) -> Result<Vec<JobEntry>> {
        match File::open(&self.path) {
            Ok(file) => parse_entries(BufReader::new(file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(storage_error("read", &error)),
        }
    }

    // The callback is a test seam, not a production flag/environment failpoint.
    // Abrupt subprocess exits can verify both sides of the publication boundary.
    fn write_atomic(
        &self,
        entries: &[JobEntry],
        mut checkpoint: impl FnMut(CommitStage, &Path) -> Result<()>,
    ) -> Result<()> {
        let mut staged = tempfile::Builder::new()
            .prefix(".jobs-")
            .suffix(".tmp")
            .tempfile_in(self.parent())
            .map_err(|error| storage_error("stage a snapshot", &error))?;
        {
            let mut writer = BufWriter::new(staged.as_file_mut());
            for entry in entries {
                serde_json::to_writer(&mut writer, entry).map_err(|error| {
                    storage_error(
                        "write its staged snapshot",
                        &io::Error::from(
                            error.io_error_kind().unwrap_or(io::ErrorKind::InvalidData),
                        ),
                    )
                })?;
                writer
                    .write_all(b"\n")
                    .map_err(|error| storage_error("write its staged snapshot", &error))?;
            }
            writer
                .flush()
                .map_err(|error| storage_error("flush its staged snapshot", &error))?;
        }
        staged
            .as_file()
            .sync_all()
            .map_err(|error| storage_error("sync its staged snapshot", &error))?;
        checkpoint(CommitStage::Staged, staged.path())?;
        let published = staged
            .persist(&self.path)
            .map_err(|error| storage_error("publish its snapshot", &error.error))?;
        checkpoint(CommitStage::Published, &self.path)?;
        // Flush the published file on Windows too. Unix additionally flushes the
        // directory entry. Filesystem/hardware power-loss guarantees vary.
        published
            .sync_all()
            .map_err(|error| storage_error("sync its published snapshot", &error))?;
        #[cfg(unix)]
        sync_directory(self.parent())?;
        // std has no portable directory fsync on Windows. The published file
        // was explicitly flushed above; do not claim stronger guarantees.
        Ok(())
    }
}

fn parse_entries(reader: impl BufRead) -> Result<Vec<JobEntry>> {
    let mut entries = Vec::new();
    let mut ids = HashSet::new();
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.map_err(|_| invalid_record(line_number, "unreadable record"))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: JobEntry = serde_json::from_str(&line)
            .map_err(|_| invalid_record(line_number, "malformed JSON or invalid receipt fields"))?;
        if entry.id.trim().is_empty() || !ids.insert(entry.id.clone()) {
            return Err(invalid_record(line_number, "empty or duplicate job ID"));
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn invalid_record(line: usize, reason: &str) -> anyhow::Error {
    FabioError::with_typed_hint(
        ErrorCode::InvalidInput,
        format!("Local receipt ledger validation failed at line {line}: {reason}. No records were loaded or changed."),
        RECOVERY_HINT,
        HintType::SemanticCorrection,
    ).set_retriable(Some(false)).into()
}

fn storage_error(action: &str, error: &io::Error) -> anyhow::Error {
    // ErrorKind is safe; OS messages/JSON parser text can contain private paths
    // or receipt values and must never leak through structured diagnostics.
    FabioError::with_typed_hint(
        ErrorCode::Unknown,
        format!(
            "Local receipt ledger: cannot {action} ({:?}).",
            error.kind()
        ),
        RECOVERY_HINT,
        HintType::SemanticCorrection,
    )
    .set_retriable(Some(false))
    .into()
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| storage_error("sync its directory", &error))
}

#[cfg(test)]
mod tests;
