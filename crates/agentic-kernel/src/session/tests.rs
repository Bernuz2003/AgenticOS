use super::{SessionRegistry, SessionState};
    use crate::storage::StorageService;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sessions_survive_reopen_without_reusing_stale_active_pid() {
        let dir = make_temp_dir("agenticos_session_registry");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let first_boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record first boot");
        let mut registry =
            SessionRegistry::load(&mut storage, first_boot.boot_id).expect("load registry");
        let session_id = registry
            .open_session(&mut storage, "Persist me across reboot", "rt-test")
            .expect("open session");
        registry
            .bind_pid(&mut storage, &session_id, "rt-test", 11)
            .expect("bind pid");

        drop(registry);
        drop(storage);

        let mut reopened_storage = StorageService::open(&db_path).expect("reopen storage");
        let second_boot = reopened_storage
            .record_kernel_boot("0.5.0-test")
            .expect("record second boot");
        let reopened_registry = SessionRegistry::load(&mut reopened_storage, second_boot.boot_id)
            .expect("load registry");

        let reopened_session = reopened_registry
            .session(&session_id)
            .expect("session should survive reboot");
        assert_eq!(reopened_session.active_pid, None);
        assert_eq!(reopened_session.state, SessionState::Idle);
        assert_eq!(reopened_registry.session_id_for_pid(11), None);

        let _ = fs::remove_dir_all(dir);
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), timestamp));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
