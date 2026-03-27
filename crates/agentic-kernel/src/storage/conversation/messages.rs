use rusqlite::{params, Transaction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredReplayMessage {
    pub(crate) role: String,
    pub(crate) kind: String,
    pub(crate) content: String,
}

pub(crate) fn next_message_ordinal(
    transaction: &Transaction<'_>,
    turn_id: i64,
) -> Result<i64, rusqlite::Error> {
    transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM session_messages WHERE turn_id = ?1",
        params![turn_id],
        |row| row.get(0),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_message(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: i64,
    pid: u64,
    ordinal: i64,
    role: &str,
    kind: &str,
    content: &str,
    created_at_ms: i64,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        r#"
        INSERT INTO session_messages (
            session_id,
            turn_id,
            pid,
            ordinal,
            role,
            kind,
            content,
            created_at_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            session_id,
            turn_id,
            pid,
            ordinal,
            role,
            kind,
            content,
            created_at_ms
        ],
    )?;
    Ok(())
}
