use super::{primary_artifact_id, StorageService, WorkflowArtifactInputRef};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn workflow_attempts_and_artifacts_survive_reopen() {
    let dir = make_temp_dir("agenticos_workflow_artifacts");
    let db_path = dir.join("agenticos.db");

    {
        let mut storage = StorageService::open(&db_path).expect("open storage");
        storage
            .insert_session("sess-research-1", "research", "idle", None, None, 1, 1)
            .expect("insert research session");
        storage
            .insert_session("sess-deliver-1", "deliver", "idle", None, None, 1, 1)
            .expect("insert deliver session");
        storage
            .begin_workflow_task_attempt(
                11,
                "research",
                1,
                Some("sess-research-1"),
                Some(42),
                1000,
                &[],
            )
            .expect("begin attempt");
        let artifact = storage
            .finalize_workflow_task_attempt(
                11,
                "research",
                1,
                "completed",
                None,
                "Structured analysis output",
                false,
                2000,
            )
            .expect("finalize attempt")
            .expect("artifact");
        assert_eq!(artifact.artifact_id, primary_artifact_id(11, "research", 1));

        storage
            .begin_workflow_task_attempt(
                11,
                "deliver",
                1,
                Some("sess-deliver-1"),
                Some(43),
                3000,
                &[WorkflowArtifactInputRef {
                    artifact_id: artifact.artifact_id.clone(),
                    producer_task_id: "research".to_string(),
                    producer_attempt: 1,
                }],
            )
            .expect("begin dependent attempt");
        storage
            .finalize_workflow_task_attempt(
                11,
                "deliver",
                1,
                "failed",
                Some("tool error"),
                "Partial draft",
                false,
                4000,
            )
            .expect("finalize failed attempt");
    }

    let reopened = StorageService::open(&db_path).expect("reopen storage");
    let workflow_io = reopened.load_workflow_io(11).expect("load workflow io");

    assert_eq!(workflow_io.attempts.len(), 2);
    assert_eq!(workflow_io.artifacts.len(), 2);
    assert_eq!(workflow_io.inputs.len(), 1);

    let deliver_attempt = workflow_io
        .attempts
        .iter()
        .find(|attempt| attempt.task_id == "deliver")
        .expect("deliver attempt");
    assert_eq!(deliver_attempt.status, "failed");
    assert_eq!(deliver_attempt.error.as_deref(), Some("tool error"));

    let deliver_artifact = workflow_io
        .artifacts
        .iter()
        .find(|artifact| artifact.producer_task_id == "deliver")
        .expect("deliver artifact");
    assert_eq!(deliver_artifact.kind, "task_output_partial");
    assert_eq!(deliver_artifact.content_text, "Partial draft");

    let input = workflow_io.inputs.first().expect("artifact input");
    assert_eq!(input.consumer_task_id, "deliver");
    assert_eq!(input.producer_task_id, "research");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn spawn_failures_are_recorded_without_overwriting_previous_attempts() {
    let dir = make_temp_dir("agenticos_workflow_spawn_failures");
    let db_path = dir.join("agenticos.db");

    let mut storage = StorageService::open(&db_path).expect("open storage");
    storage
        .record_workflow_task_spawn_failure(7, "research", 1, "routing failed", 1000)
        .expect("record first failure");
    storage
        .record_workflow_task_spawn_failure(7, "research", 2, "quota denied", 2000)
        .expect("record second failure");

    let workflow_io = storage.load_workflow_io(7).expect("load workflow io");
    assert_eq!(workflow_io.attempts.len(), 2);
    assert!(workflow_io.artifacts.is_empty());

    let latest_attempt = storage
        .workflow_task_attempt_by_key(7, "research", 2)
        .expect("load attempt")
        .expect("attempt exists");
    assert_eq!(latest_attempt.error.as_deref(), Some("quota denied"));
    assert_eq!(latest_attempt.completed_at_ms, Some(2000));

    let _ = fs::remove_dir_all(dir);
}

fn make_temp_dir(prefix: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), timestamp));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
