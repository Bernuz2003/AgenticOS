use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::config::kernel_config;
use crate::storage::{current_timestamp_ms, StorageService};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoreDumpRetentionPolicy {
    pub(crate) max_files: Option<usize>,
    pub(crate) max_age_ms: Option<i64>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct CoreDumpRetentionOutcome {
    pub(crate) pruned_dump_ids: Vec<String>,
    pub(crate) reclaimed_bytes: usize,
    pub(crate) stale_index_entries: usize,
}

pub(crate) fn configured_retention_policy() -> CoreDumpRetentionPolicy {
    let config = &kernel_config().core_dump;
    CoreDumpRetentionPolicy {
        max_files: (config.retention_max_files > 0).then_some(config.retention_max_files),
        max_age_ms: (config.retention_max_age_hours > 0)
            .then_some((config.retention_max_age_hours as i64) * 60 * 60 * 1000),
    }
}

pub(crate) fn apply_core_dump_retention(
    storage: &mut StorageService,
    policy: CoreDumpRetentionPolicy,
) -> Result<CoreDumpRetentionOutcome, String> {
    let records = storage
        .load_all_core_dump_records()
        .map_err(|err| err.to_string())?;
    if records.is_empty() {
        return Ok(CoreDumpRetentionOutcome::default());
    }

    let age_cutoff_ms = policy
        .max_age_ms
        .and_then(|max_age_ms| current_timestamp_ms().checked_sub(max_age_ms));
    let mut prune_ids = HashSet::new();
    let mut stale_index_entries = 0usize;

    for (index, record) in records.iter().enumerate() {
        let missing_artifact = !Path::new(&record.path).exists();
        if missing_artifact {
            stale_index_entries += 1;
            prune_ids.insert(record.dump_id.clone());
            continue;
        }

        if policy.max_files.is_some_and(|max_files| index >= max_files) {
            prune_ids.insert(record.dump_id.clone());
            continue;
        }

        if age_cutoff_ms.is_some_and(|cutoff_ms| record.created_at_ms < cutoff_ms) {
            prune_ids.insert(record.dump_id.clone());
        }
    }

    if prune_ids.is_empty() {
        return Ok(CoreDumpRetentionOutcome::default());
    }

    let mut outcome = CoreDumpRetentionOutcome {
        stale_index_entries,
        ..CoreDumpRetentionOutcome::default()
    };

    for record in records {
        if !prune_ids.contains(&record.dump_id) {
            continue;
        }

        if fs::remove_file(&record.path).is_ok() {
            outcome.reclaimed_bytes = outcome.reclaimed_bytes.saturating_add(record.bytes);
        }
        storage
            .delete_core_dump_record(&record.dump_id)
            .map_err(|err| err.to_string())?;
        outcome.pruned_dump_ids.push(record.dump_id);
    }

    Ok(outcome)
}
