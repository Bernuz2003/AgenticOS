use super::*;
    

    fn make_test_snapshot() -> KernelSnapshot {
        KernelSnapshot {
            timestamp: "epoch_1709600000".to_string(),
            version: "0.5.0".to_string(),
            active_family: "llama".to_string(),
            selected_model: Some("llama3.1-8b".to_string()),
            generation: Some(GenerationSnapshot {
                temperature: 0.7,
                top_p: 0.9,
                seed: 42,
                max_tokens: 256,
            }),
            processes: vec![
                ProcessSnapshot {
                    pid: 1,
                    owner_id: 10,
                    state: "Running".to_string(),
                    token_count: 128,
                    max_tokens: 256,
                    context_policy: ContextPolicy::from_kernel_defaults(),
                    context_state: ContextState::default(),
                },
                ProcessSnapshot {
                    pid: 2,
                    owner_id: 11,
                    state: "Paused".to_string(),
                    token_count: 64,
                    max_tokens: 512,
                    context_policy: ContextPolicy::from_kernel_defaults(),
                    context_state: ContextState::default(),
                },
            ],
            scheduler: SchedulerStateSnapshot {
                entries: vec![SchedulerEntrySnapshot {
                    pid: 1,
                    priority: "high".to_string(),
                    workload: "Code".to_string(),
                    max_tokens: 4096,
                    max_syscalls: 16,
                    tokens_generated: 100,
                    syscalls_used: 3,
                    elapsed_secs: 12.5,
                }],
            },
            metrics: MetricsSnapshot {
                uptime_secs: 3600,
                total_commands: 42,
                total_errors: 1,
                total_exec_started: 10,
                total_signals: 2,
            },
            memory: MemoryCountersSnapshot {
                active: true,
                total_blocks: 256,
                free_blocks: 200,
                allocated_tensors: 5,
                tracked_pids: 2,
                alloc_bytes: 1024000,
                evictions: 3,
                swap_count: 1,
                swap_faults: 0,
                oom_events: 0,
            },
        }
    }

    #[test]
    fn serialization_roundtrip() {
        let snap = make_test_snapshot();
        let json = serde_json::to_string_pretty(&snap).expect("serialize");
        let restored: KernelSnapshot = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.version, "0.5.0");
        assert_eq!(restored.processes.len(), 2);
        assert_eq!(restored.processes[0].pid, 1);
        assert_eq!(restored.scheduler.entries.len(), 1);
        assert_eq!(restored.scheduler.entries[0].priority, "high");
        assert_eq!(restored.metrics.total_commands, 42);
        assert_eq!(restored.memory.total_blocks, 256);
        assert_eq!(restored.generation.as_ref().unwrap().temperature, 0.7);
    }

    #[test]
    fn save_and_load_checkpoint_atomic() {
        let snap = make_test_snapshot();
        let dir = PathBuf::from(format!(
            "workspace/test_checkpoint_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("checkpoint.json");

        let msg = save_checkpoint(&snap, &path).expect("save");
        assert!(msg.contains("checkpoint saved"));
        assert!(path.exists());

        // tmp must not linger
        assert!(!path.with_extension("json.tmp").exists());

        let loaded = load_checkpoint(&path).expect("load");
        assert_eq!(loaded.version, snap.version);
        assert_eq!(loaded.processes.len(), snap.processes.len());
        assert_eq!(loaded.scheduler.entries.len(), snap.scheduler.entries.len());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_corrupt_checkpoint_returns_error() {
        let dir = PathBuf::from(format!(
            "workspace/test_checkpoint_corrupt_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("checkpoint.json");
        fs::write(&path, b"{{not valid json").unwrap();

        let err = load_checkpoint(&path).expect_err("should fail on corrupt data");
        assert!(err.contains("corrupt"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_missing_checkpoint_returns_error() {
        let err = load_checkpoint(Path::new("workspace/nonexistent_checkpoint.json"))
            .expect_err("should fail on missing file");
        assert!(err.contains("cannot read"));
    }

    #[test]
    fn now_timestamp_contains_epoch() {
        let ts = now_timestamp();
        assert!(ts.starts_with("epoch_"));
    }
