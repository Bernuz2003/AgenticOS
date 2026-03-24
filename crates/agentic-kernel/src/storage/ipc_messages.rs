use rusqlite::params;

use super::service::{current_timestamp_ms, StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct IpcMailboxSelector {
    pub orchestration_id: Option<u64>,
    pub receiver_pid: Option<u64>,
    pub receiver_task_id: Option<String>,
    pub receiver_role: Option<String>,
    pub channel: Option<String>,
}

impl IpcMailboxSelector {
    pub(crate) fn matches(&self, message: &StoredIpcMessage) -> bool {
        if let Some(orchestration_id) = self.orchestration_id {
            if message.orchestration_id != Some(orchestration_id) {
                return false;
            }
        }

        let pid_match = self
            .receiver_pid
            .is_some_and(|receiver_pid| message.receiver_pid == Some(receiver_pid));
        let task_match = self
            .receiver_task_id
            .as_ref()
            .is_some_and(|receiver_task_id| {
                message.receiver_task_id.as_ref() == Some(receiver_task_id)
            });
        let role_match = self
            .receiver_role
            .as_ref()
            .is_some_and(|receiver_role| message.receiver_role.as_ref() == Some(receiver_role));
        let channel_match = self
            .channel
            .as_ref()
            .is_some_and(|channel| message.channel.as_ref() == Some(channel));

        pid_match || task_match || role_match || channel_match
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewIpcMessage {
    pub message_id: String,
    pub orchestration_id: Option<u64>,
    pub sender_pid: Option<u64>,
    pub sender_task_id: Option<String>,
    pub sender_attempt: Option<u32>,
    pub receiver_pid: Option<u64>,
    pub receiver_task_id: Option<String>,
    pub receiver_attempt: Option<u32>,
    pub receiver_role: Option<String>,
    pub message_type: String,
    pub channel: Option<String>,
    pub payload_preview: String,
    pub payload_text: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredIpcMessage {
    pub message_id: String,
    pub orchestration_id: Option<u64>,
    pub sender_pid: Option<u64>,
    pub sender_task_id: Option<String>,
    pub sender_attempt: Option<u32>,
    pub receiver_pid: Option<u64>,
    pub receiver_task_id: Option<String>,
    pub receiver_attempt: Option<u32>,
    pub receiver_role: Option<String>,
    pub message_type: String,
    pub channel: Option<String>,
    pub payload_preview: String,
    pub payload_text: String,
    pub status: String,
    pub created_at_ms: i64,
    pub delivered_at_ms: Option<i64>,
    pub consumed_at_ms: Option<i64>,
    pub failed_at_ms: Option<i64>,
}

impl StorageService {
    pub(crate) fn record_ipc_message(
        &mut self,
        message: &NewIpcMessage,
    ) -> Result<StoredIpcMessage, StorageError> {
        let created_at_ms = current_timestamp_ms();
        self.connection.execute(
            r#"
            INSERT INTO ipc_messages (
                message_id,
                orchestration_id,
                sender_pid,
                sender_task_id,
                sender_attempt,
                receiver_pid,
                receiver_task_id,
                receiver_attempt,
                receiver_role,
                message_type,
                channel,
                payload_preview,
                payload_text,
                status,
                created_at_ms,
                delivered_at_ms,
                consumed_at_ms,
                failed_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL, NULL, NULL)
            "#,
            params![
                message.message_id,
                message.orchestration_id,
                message.sender_pid,
                message.sender_task_id,
                message.sender_attempt,
                message.receiver_pid,
                message.receiver_task_id,
                message.receiver_attempt,
                message.receiver_role,
                message.message_type,
                message.channel,
                message.payload_preview,
                message.payload_text,
                message.status,
                created_at_ms,
            ],
        )?;

        Ok(StoredIpcMessage {
            message_id: message.message_id.clone(),
            orchestration_id: message.orchestration_id,
            sender_pid: message.sender_pid,
            sender_task_id: message.sender_task_id.clone(),
            sender_attempt: message.sender_attempt,
            receiver_pid: message.receiver_pid,
            receiver_task_id: message.receiver_task_id.clone(),
            receiver_attempt: message.receiver_attempt,
            receiver_role: message.receiver_role.clone(),
            message_type: message.message_type.clone(),
            channel: message.channel.clone(),
            payload_preview: message.payload_preview.clone(),
            payload_text: message.payload_text.clone(),
            status: message.status.clone(),
            created_at_ms,
            delivered_at_ms: None,
            consumed_at_ms: None,
            failed_at_ms: None,
        })
    }

    pub(crate) fn update_ipc_message_status(
        &mut self,
        message_id: &str,
        status: &str,
        delivered_at_ms: Option<i64>,
        consumed_at_ms: Option<i64>,
        failed_at_ms: Option<i64>,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            UPDATE ipc_messages
            SET status = ?2,
                delivered_at_ms = COALESCE(?3, delivered_at_ms),
                consumed_at_ms = COALESCE(?4, consumed_at_ms),
                failed_at_ms = COALESCE(?5, failed_at_ms)
            WHERE message_id = ?1
            "#,
            params![
                message_id,
                status,
                delivered_at_ms,
                consumed_at_ms,
                failed_at_ms
            ],
        )?;
        Ok(())
    }

    pub(crate) fn load_ipc_messages_for_orchestration(
        &self,
        orchestration_id: u64,
    ) -> Result<Vec<StoredIpcMessage>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                message_id,
                orchestration_id,
                sender_pid,
                sender_task_id,
                sender_attempt,
                receiver_pid,
                receiver_task_id,
                receiver_attempt,
                receiver_role,
                message_type,
                channel,
                payload_preview,
                payload_text,
                status,
                created_at_ms,
                delivered_at_ms,
                consumed_at_ms,
                failed_at_ms
            FROM ipc_messages
            WHERE orchestration_id = ?1
            ORDER BY created_at_ms DESC, message_id DESC
            "#,
        )?;
        let rows = statement.query_map(params![orchestration_id], |row| {
            Ok(StoredIpcMessage {
                message_id: row.get(0)?,
                orchestration_id: row.get(1)?,
                sender_pid: row.get(2)?,
                sender_task_id: row.get(3)?,
                sender_attempt: row.get(4)?,
                receiver_pid: row.get(5)?,
                receiver_task_id: row.get(6)?,
                receiver_attempt: row.get(7)?,
                receiver_role: row.get(8)?,
                message_type: row.get(9)?,
                channel: row.get(10)?,
                payload_preview: row.get(11)?,
                payload_text: row.get(12)?,
                status: row.get(13)?,
                created_at_ms: row.get(14)?,
                delivered_at_ms: row.get(15)?,
                consumed_at_ms: row.get(16)?,
                failed_at_ms: row.get(17)?,
            })
        })?;

        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        Ok(values)
    }

    pub(crate) fn load_ipc_mailbox_messages(
        &self,
        selector: &IpcMailboxSelector,
        include_delivered: bool,
        limit: usize,
    ) -> Result<Vec<StoredIpcMessage>, StorageError> {
        let query_limit = limit.clamp(1, 64) as i64;
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                message_id,
                orchestration_id,
                sender_pid,
                sender_task_id,
                sender_attempt,
                receiver_pid,
                receiver_task_id,
                receiver_attempt,
                receiver_role,
                message_type,
                channel,
                payload_preview,
                payload_text,
                status,
                created_at_ms,
                delivered_at_ms,
                consumed_at_ms,
                failed_at_ms
            FROM ipc_messages
            WHERE status IN ('queued', 'pending', 'delivered')
              AND (
                    (?1 IS NOT NULL AND receiver_pid = ?1)
                 OR (?2 IS NOT NULL AND orchestration_id = ?3 AND receiver_task_id = ?2)
                 OR (?4 IS NOT NULL AND orchestration_id = ?3 AND receiver_role = ?4)
                 OR (?5 IS NOT NULL AND orchestration_id = ?3 AND channel = ?5)
              )
            ORDER BY created_at_ms ASC, message_id ASC
            LIMIT ?6
            "#,
        )?;
        let rows = statement.query_map(
            params![
                selector.receiver_pid,
                selector.receiver_task_id,
                selector.orchestration_id,
                selector.receiver_role,
                selector.channel,
                query_limit
            ],
            |row| {
                Ok(StoredIpcMessage {
                    message_id: row.get(0)?,
                    orchestration_id: row.get(1)?,
                    sender_pid: row.get(2)?,
                    sender_task_id: row.get(3)?,
                    sender_attempt: row.get(4)?,
                    receiver_pid: row.get(5)?,
                    receiver_task_id: row.get(6)?,
                    receiver_attempt: row.get(7)?,
                    receiver_role: row.get(8)?,
                    message_type: row.get(9)?,
                    channel: row.get(10)?,
                    payload_preview: row.get(11)?,
                    payload_text: row.get(12)?,
                    status: row.get(13)?,
                    created_at_ms: row.get(14)?,
                    delivered_at_ms: row.get(15)?,
                    consumed_at_ms: row.get(16)?,
                    failed_at_ms: row.get(17)?,
                })
            },
        )?;

        let mut values = Vec::new();
        for row in rows {
            let message = row?;
            if matches!(message.status.as_str(), "queued" | "pending") || include_delivered {
                values.push(message);
            }
        }
        Ok(values)
    }

    pub(crate) fn load_ipc_messages_by_ids(
        &self,
        message_ids: &[String],
    ) -> Result<Vec<StoredIpcMessage>, StorageError> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = std::iter::repeat_n("?", message_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT
                message_id,
                orchestration_id,
                sender_pid,
                sender_task_id,
                sender_attempt,
                receiver_pid,
                receiver_task_id,
                receiver_attempt,
                receiver_role,
                message_type,
                channel,
                payload_preview,
                payload_text,
                status,
                created_at_ms,
                delivered_at_ms,
                consumed_at_ms,
                failed_at_ms
            FROM ipc_messages
            WHERE message_id IN ({placeholders})
            ORDER BY created_at_ms ASC, message_id ASC
            "#
        );

        let mut statement = self.connection.prepare(&sql)?;
        let params = rusqlite::params_from_iter(message_ids.iter());
        let rows = statement.query_map(params, |row| {
            Ok(StoredIpcMessage {
                message_id: row.get(0)?,
                orchestration_id: row.get(1)?,
                sender_pid: row.get(2)?,
                sender_task_id: row.get(3)?,
                sender_attempt: row.get(4)?,
                receiver_pid: row.get(5)?,
                receiver_task_id: row.get(6)?,
                receiver_attempt: row.get(7)?,
                receiver_role: row.get(8)?,
                message_type: row.get(9)?,
                channel: row.get(10)?,
                payload_preview: row.get(11)?,
                payload_text: row.get(12)?,
                status: row.get(13)?,
                created_at_ms: row.get(14)?,
                delivered_at_ms: row.get(15)?,
                consumed_at_ms: row.get(16)?,
                failed_at_ms: row.get(17)?,
            })
        })?;

        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        Ok(values)
    }

    pub(crate) fn delete_ipc_messages_for_orchestration(
        &mut self,
        orchestration_id: u64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "DELETE FROM ipc_messages WHERE orchestration_id = ?1",
            params![orchestration_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "ipc_messages_tests.rs"]
mod tests;
