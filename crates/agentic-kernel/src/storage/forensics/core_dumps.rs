use rusqlite::params;

use crate::storage::{StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewCoreDumpRecord {
    pub(crate) dump_id: String,
    pub(crate) created_at_ms: i64,
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) reason: String,
    pub(crate) fidelity: String,
    pub(crate) path: String,
    pub(crate) bytes: usize,
    pub(crate) sha256: String,
    pub(crate) note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredCoreDumpRecord {
    pub(crate) dump_id: String,
    pub(crate) created_at_ms: i64,
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) reason: String,
    pub(crate) fidelity: String,
    pub(crate) path: String,
    pub(crate) bytes: usize,
    pub(crate) sha256: String,
    pub(crate) note: Option<String>,
}

impl StorageService {
    pub(crate) fn record_core_dump(
        &mut self,
        record: &NewCoreDumpRecord,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO core_dump_index (
                dump_id,
                created_at_ms,
                session_id,
                pid,
                reason,
                fidelity,
                path,
                bytes,
                sha256,
                note
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                record.dump_id,
                record.created_at_ms,
                record.session_id,
                record.pid,
                record.reason,
                record.fidelity,
                record.path,
                record.bytes as i64,
                record.sha256,
                record.note,
            ],
        )?;

        Ok(())
    }

    pub(crate) fn load_core_dump_index(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredCoreDumpRecord>, StorageError> {
        let limit = limit.min(i64::MAX as usize) as i64;
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                dump_id,
                created_at_ms,
                session_id,
                pid,
                reason,
                fidelity,
                path,
                bytes,
                sha256,
                note
            FROM core_dump_index
            ORDER BY created_at_ms DESC, dump_id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = statement.query_map(params![limit], map_core_dump_row)?;

        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        Ok(values)
    }

    pub(crate) fn load_all_core_dump_records(
        &self,
    ) -> Result<Vec<StoredCoreDumpRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                dump_id,
                created_at_ms,
                session_id,
                pid,
                reason,
                fidelity,
                path,
                bytes,
                sha256,
                note
            FROM core_dump_index
            ORDER BY created_at_ms DESC, dump_id DESC
            "#,
        )?;
        let rows = statement.query_map([], map_core_dump_row)?;

        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        Ok(values)
    }

    pub(crate) fn core_dump_record(
        &self,
        dump_id: &str,
    ) -> Result<Option<StoredCoreDumpRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                dump_id,
                created_at_ms,
                session_id,
                pid,
                reason,
                fidelity,
                path,
                bytes,
                sha256,
                note
            FROM core_dump_index
            WHERE dump_id = ?1
            LIMIT 1
            "#,
        )?;

        let mut rows = statement.query_map(params![dump_id], map_core_dump_row)?;
        rows.next().transpose().map_err(StorageError::from)
    }

    pub(crate) fn delete_core_dump_record(&mut self, dump_id: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "DELETE FROM core_dump_index WHERE dump_id = ?1",
            params![dump_id],
        )?;
        Ok(())
    }
}

fn map_core_dump_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredCoreDumpRecord> {
    Ok(StoredCoreDumpRecord {
        dump_id: row.get(0)?,
        created_at_ms: row.get(1)?,
        session_id: row.get(2)?,
        pid: row.get(3)?,
        reason: row.get(4)?,
        fidelity: row.get(5)?,
        path: row.get(6)?,
        bytes: row.get::<_, i64>(7)? as usize,
        sha256: row.get(8)?,
        note: row.get(9)?,
    })
}
