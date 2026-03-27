use rusqlite::{params, OptionalExtension, Transaction};

pub(super) fn next_turn_index(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<i64, rusqlite::Error> {
    transaction.query_row(
        "SELECT COALESCE(MAX(turn_index), 0) + 1 FROM session_turns WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )
}

pub(super) fn active_run_id_for_session_pid(
    transaction: &Transaction<'_>,
    session_id: &str,
    pid: u64,
) -> Result<Option<i64>, rusqlite::Error> {
    transaction
        .query_row(
            r#"
            SELECT run_id
            FROM process_runs
            WHERE session_id = ?1
              AND pid = ?2
              AND ended_at_ms IS NULL
            ORDER BY run_id DESC
            LIMIT 1
            "#,
            params![session_id, pid],
            |row| row.get(0),
        )
        .optional()
}

pub(super) fn latest_turn_id_for_session(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    transaction
        .query_row(
            r#"
            SELECT turn_id
            FROM session_turns
            WHERE session_id = ?1
            ORDER BY turn_index DESC, turn_id DESC
            LIMIT 1
            "#,
            params![session_id],
            |row| row.get(0),
        )
        .optional()
}

pub(super) fn turn_identity(
    transaction: &Transaction<'_>,
    turn_id: i64,
) -> Result<Option<(String, u64)>, rusqlite::Error> {
    transaction
        .query_row(
            "SELECT session_id, pid FROM session_turns WHERE turn_id = ?1",
            params![turn_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
}

pub(super) fn assistant_message_id_for_turn(
    transaction: &Transaction<'_>,
    turn_id: i64,
) -> Result<Option<i64>, rusqlite::Error> {
    transaction
        .query_row(
            r#"
            SELECT message_id
            FROM session_messages
            WHERE turn_id = ?1
              AND role = 'assistant'
            ORDER BY ordinal ASC, message_id ASC
            LIMIT 1
            "#,
            params![turn_id],
            |row| row.get(0),
        )
        .optional()
}
