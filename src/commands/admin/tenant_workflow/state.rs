use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::errors::{ErrorCode, FabioError};

use super::Operation;
use super::model::{self, Desired, Plan, Setting, VERSION, bytes_digest, digest, invalid, parse};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Snapshot,
    Apply,
    Finalize,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Pending,
    Completed,
    Skipped,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_field_names)]
pub(super) struct Checkpoint {
    pub schema_version: u32,
    pub kind: String,
    pub operation: Operation,
    pub phase: Phase,
    pub tenant_id: String,
    pub endpoint: String,
    pub checkpoint: PathBuf,
    pub input_file: Option<PathBuf>,
    pub input_digest: Option<String>,
    pub out: Option<PathBuf>,
    pub desired: Option<Desired>,
    pub plan: Option<Plan>,
    pub snapshot: Vec<Setting>,
    pub next_page: Option<String>,
    pub visited_pages: BTreeSet<String>,
    pub pages: usize,
    pub outcomes: Vec<Outcome>,
    pub retry_after: Option<String>,
    pub retry_at: Option<String>,
    pub artifact_digest: Option<String>,
    pub integrity: String,
}

impl Checkpoint {
    pub fn save(&mut self) -> Result<()> {
        self.integrity.clear();
        self.integrity = digest(self)?;
        atomic_write(
            &self.checkpoint,
            serde_yaml::to_string(self)?.as_bytes(),
            true,
        )
    }

    pub fn validate(&self, operation: Operation, path: &Path) -> Result<()> {
        model::binding(self.schema_version, &self.tenant_id, &self.endpoint)?;
        if self.kind != "tenant-settings-checkpoint"
            || self.operation != operation
            || self.checkpoint != path
        {
            return Err(invalid("Checkpoint operation or path mismatch"));
        }
        let mut copy = self.clone();
        copy.integrity.clear();
        if digest(&copy)? != self.integrity {
            return Err(invalid(
                "Checkpoint integrity mismatch; restore the last intact checkpoint",
            ));
        }
        if self.pages != self.visited_pages.len() {
            return Err(invalid("Invalid snapshot pagination history"));
        }
        let mut names = BTreeSet::new();
        for setting in &self.snapshot {
            model::setting_name(&setting.setting_name)?;
            let mut values = setting.values.clone();
            values.normalize()?;
            if !names.insert(&setting.setting_name)
                || values.enabled.is_none()
                || values != setting.values
            {
                return Err(invalid("Invalid or duplicate checkpoint snapshot entry"));
            }
        }
        match operation {
            Operation::Export => {
                if self.input_file.is_some()
                    || self.input_digest.is_some()
                    || self.desired.is_some()
                    || self.plan.is_some()
                    || self.out.is_none()
                    || !self.outcomes.is_empty()
                    || self.phase == Phase::Apply
                {
                    return Err(invalid("Invalid export checkpoint"));
                }
            }
            Operation::Plan => {
                let mut desired = self
                    .desired
                    .clone()
                    .ok_or_else(|| invalid("Missing desired state"))?;
                desired.validate()?;
                if desired.tenant_id != self.tenant_id
                    || desired.endpoint != self.endpoint
                    || self.out.is_none()
                    || !self.outcomes.is_empty()
                    || self.phase == Phase::Apply
                {
                    return Err(invalid("Invalid planning checkpoint"));
                }
                if let Some(plan) = &self.plan {
                    plan.validate()?;
                }
            }
            Operation::Apply => {
                let plan = self.plan.as_ref().ok_or_else(|| invalid("Missing plan"))?;
                plan.validate()?;
                if plan.tenant_id != self.tenant_id
                    || plan.endpoint != self.endpoint
                    || self.out.is_some()
                    || self.desired.is_some()
                    || plan.updates.len() != self.outcomes.len()
                {
                    return Err(invalid("Invalid execution checkpoint"));
                }
                let mut pending = false;
                let mut uncertain = 0;
                for outcome in &self.outcomes {
                    match outcome {
                        Outcome::Pending | Outcome::Uncertain => {
                            pending = true;
                            uncertain += usize::from(*outcome == Outcome::Uncertain);
                        }
                        Outcome::Completed if pending => {
                            return Err(invalid("Non-sequential checkpoint acknowledgments"));
                        }
                        _ => {}
                    }
                }
                if uncertain > 1 || (self.phase == Phase::Completed && pending) {
                    return Err(invalid("Invalid uncertain or terminal checkpoint state"));
                }
            }
        }
        self.validate_input()
    }

    fn validate_input(&self) -> Result<()> {
        if self.operation != Operation::Export {
            let path = self
                .input_file
                .as_ref()
                .ok_or_else(|| invalid("Missing checkpoint input path"))?;
            let bytes = read(path)?;
            if self.input_digest.as_deref() != Some(bytes_digest(&bytes).as_str()) {
                return Err(invalid(
                    "Input file changed since checkpoint creation; use the original file or create a fresh workflow",
                ));
            }
            match self.operation {
                Operation::Plan => {
                    let mut desired: Desired = parse(&bytes)?;
                    desired.validate()?;
                    if digest(&desired)? != digest(&self.desired)? {
                        return Err(invalid("Checkpoint desired state differs from its input"));
                    }
                }
                Operation::Apply => {
                    let plan: Plan = parse(&bytes)?;
                    plan.validate()?;
                    if digest(&plan)? != digest(&self.plan)? {
                        return Err(invalid("Checkpoint plan differs from its input"));
                    }
                }
                Operation::Export => {}
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> serde_json::Value {
        let total = self
            .plan
            .as_ref()
            .map_or(self.snapshot.len(), |p| p.updates.len());
        let count = |value| self.outcomes.iter().filter(|s| **s == value).count();
        let destructive = self.plan.as_ref().is_some_and(|p| !p.updates.is_empty());
        json!({
            "operation": self.operation,
            "status": if self.phase == Phase::Completed { "completed" } else { "incomplete" },
            "tenantId": self.tenant_id,
            "out": self.out,
            "checkpoint": self.checkpoint,
            "pages": self.pages,
            "total": total,
            "completed": count(Outcome::Completed),
            "skipped": count(Outcome::Skipped),
            "pending": count(Outcome::Pending),
            "uncertain": count(Outcome::Uncertain),
            "unchanged": self.plan.as_ref().map_or(0, |p| p.unchanged),
            "destructive": destructive,
            "warnings": if destructive { vec![model::WARNING] } else { vec![] },
        })
    }
}

pub(super) fn read(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| invalid(format!("Cannot read {}: {e}", path.display())))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 32 * 1024 * 1024
    {
        return Err(invalid(format!(
            "Expected a regular file of at most 32 MiB: {}",
            path.display()
        )));
    }
    fs::read(path).map_err(|e| invalid(format!("Cannot read {}: {e}", path.display())))
}

pub(super) fn resolved(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path).map_err(|e| invalid(format!("Invalid path: {e}")))?;
    if absolute.file_name().is_none() {
        return Err(invalid("A file path, not a directory, is required"));
    }
    let parent = absolute
        .parent()
        .ok_or_else(|| invalid("Missing parent directory"))?;
    let parent = parent.canonicalize().map_err(|e| {
        invalid(format!(
            "Invalid parent directory {}: {e}",
            parent.display()
        ))
    })?;
    let result = parent.join(
        absolute
            .file_name()
            .ok_or_else(|| invalid("Missing filename"))?,
    );
    if let Ok(metadata) = fs::symlink_metadata(&result) {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid(format!(
                "Refusing a non-regular file or symlink: {}",
                result.display()
            )));
        }
        return result.canonicalize().map_err(Into::into);
    }
    Ok(result)
}

pub(super) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

pub(super) fn validate_paths(checkpoint: &Checkpoint, fresh: bool) -> Result<()> {
    let paths: Vec<_> = [
        &Some(checkpoint.checkpoint.clone()),
        &checkpoint.input_file,
        &checkpoint.out,
    ]
    .into_iter()
    .filter_map(Clone::clone)
    .collect();
    let mut unique = BTreeSet::new();
    for path in &paths {
        if resolved(path)? != *path || !unique.insert(path.clone()) {
            return Err(invalid(
                "Input, output and checkpoint paths must be distinct resolved file paths",
            ));
        }
    }
    for path in &paths {
        if unique.contains(&sibling(path, ".lock")) {
            return Err(invalid(
                "A workflow file collides with a single-writer lock",
            ));
        }
    }
    if fresh {
        for path in [&Some(checkpoint.checkpoint.clone()), &checkpoint.out]
            .into_iter()
            .flatten()
        {
            if path.exists() {
                return Err(invalid(format!(
                    "Refusing existing file {}; use --continue for a checkpoint or choose a new destination",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

pub(super) struct Locks {
    _files: Vec<File>,
}

impl Locks {
    pub fn acquire(checkpoint: &Checkpoint) -> Result<Self> {
        let mut paths = BTreeSet::from([checkpoint.checkpoint.clone()]);
        if let Some(out) = &checkpoint.out {
            paths.insert(out.clone());
        }
        // A distinct checkpoint must not allow concurrent execution of the same plan.
        if checkpoint.operation == Operation::Apply
            && let Some(input) = &checkpoint.input_file
        {
            paths.insert(input.clone());
        }
        let mut files = Vec::new();
        for path in paths {
            let path = sibling(&path, ".lock");
            resolved(&path)?;
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(|e| {
                    invalid(format!("Cannot open workflow lock {}: {e}", path.display()))
                })?;
            fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
                FabioError::with_hint(
                    ErrorCode::Conflict,
                    "Another process owns this workflow, output, or plan",
                    "Wait for the active process to exit; do not delete its lock file.",
                )
            })?;
            files.push(file);
        }
        Ok(Self { _files: files })
    }
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(invalid(
            "Workflow files cannot exceed 32 MiB; no further requests were sent",
        ));
    }
    resolved(path)?;
    let staging = sibling(path, &format!(".pending-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if replace {
            fs::rename(&staging, path)?;
        } else {
            // Hard-link publication is atomic and never clobbers a file created
            // by another writer between validation and publication.
            fs::hard_link(&staging, path)?;
            fs::remove_file(&staging)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(staging);
    }
    result.map_err(|e| {
        invalid(format!(
            "Cannot durably save {}: {e}. No further requests were sent.",
            path.display()
        ))
    })
}

pub(super) fn fresh(
    operation: Operation,
    input: Option<&str>,
    out: Option<&str>,
    continuation: Option<&str>,
) -> Result<Checkpoint> {
    let input_file = input.map(|p| resolved(Path::new(p))).transpose()?;
    let out = out.map(|p| resolved(Path::new(p))).transpose()?;
    let anchor = out
        .as_ref()
        .or(input_file.as_ref())
        .ok_or_else(|| invalid("Missing input/output file"))?;
    let checkpoint = match continuation {
        Some(path) => resolved(Path::new(path))?,
        None => sibling(
            anchor,
            match operation {
                Operation::Export => ".export.checkpoint.yaml",
                Operation::Plan => ".plan.checkpoint.yaml",
                Operation::Apply => ".apply.checkpoint.yaml",
            },
        ),
    };
    let mut cp = Checkpoint {
        schema_version: VERSION,
        kind: "tenant-settings-checkpoint".into(),
        operation,
        phase: Phase::Snapshot,
        tenant_id: String::new(),
        endpoint: String::new(),
        checkpoint,
        input_file,
        input_digest: None,
        out,
        desired: None,
        plan: None,
        snapshot: Vec::new(),
        next_page: None,
        visited_pages: BTreeSet::new(),
        pages: 0,
        outcomes: Vec::new(),
        retry_after: None,
        retry_at: None,
        artifact_digest: None,
        integrity: String::new(),
    };
    validate_paths(&cp, true)?;
    if let Some(path) = &cp.input_file {
        let bytes = read(path)?;
        cp.input_digest = Some(bytes_digest(&bytes));
        match operation {
            Operation::Plan => {
                let mut desired: Desired = parse(&bytes)?;
                desired.validate()?;
                cp.tenant_id.clone_from(&desired.tenant_id);
                cp.endpoint.clone_from(&desired.endpoint);
                cp.desired = Some(desired);
            }
            Operation::Apply => {
                let plan: Plan = parse(&bytes)?;
                plan.validate()?;
                cp.tenant_id.clone_from(&plan.tenant_id);
                cp.endpoint.clone_from(&plan.endpoint);
                cp.outcomes = vec![Outcome::Pending; plan.updates.len()];
                cp.plan = Some(plan);
            }
            Operation::Export => return Err(invalid("Export does not accept input")),
        }
    }
    Ok(cp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_replace_and_lock_exclusion() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let out = dir.path().join("export.yaml");
        let cp = fresh(Operation::Export, None, out.to_str(), None).unwrap();
        let lock = Locks::acquire(&cp).unwrap();
        assert!(Locks::acquire(&cp).is_err());
        atomic_write(&out, b"old", false).unwrap();
        assert!(atomic_write(&out, b"clobber", false).is_err());
        assert_eq!(read(&out).unwrap(), b"old");
        atomic_write(&out, b"new", true).unwrap();
        assert_eq!(read(&out).unwrap(), b"new");
        drop(lock);
        assert!(Locks::acquire(&cp).is_ok());
    }

    #[test]
    fn test_path_collisions_and_unwritable_destination() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let out = dir.path().join("export.yaml");
        assert!(fresh(Operation::Export, None, out.to_str(), out.to_str()).is_err());
        let cp_lock = sibling(
            &sibling(&resolved(&out).unwrap(), ".checkpoint.yaml"),
            ".lock",
        );
        assert!(
            fresh(
                Operation::Export,
                None,
                cp_lock.to_str(),
                sibling(&resolved(&out).unwrap(), ".checkpoint.yaml").to_str()
            )
            .is_err()
        );
        assert!(atomic_write(&dir.path().join("missing").join("checkpoint"), b"x", false).is_err());
    }
}
