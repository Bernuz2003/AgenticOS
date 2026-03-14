/// Unit tests for resource governor and admission logic.
    use super::{ResourceGovernor, ResourceGovernorError};
    use crate::config::ResourceGovernorConfig;
    use crate::backend::{resolve_driver_for_model, TestExternalEndpointOverrideGuard};
    use crate::model_catalog::ResolvedModelTarget;
    use crate::prompting::PromptFamily;
    use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
    use crate::session::SessionRegistry;
    use crate::storage::StorageService;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::Tokenizer;

    #[test]
    fn admission_fits_immediately_when_budget_allows_it() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
        let dir = make_temp_dir("agenticos-resource-governor-fit");
        let db_path = dir.join("agenticos.db");
        let model_path = write_model_file(&dir, "fit.gguf", 2 * 1024 * 1024);
        let tokenizer_path = write_test_tokenizer(&dir);
        let target = local_target(&model_path, &tokenizer_path);
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let runtime_registry = RuntimeRegistry::load(&mut storage).expect("load registry");
        let session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut governor = ResourceGovernor::load(
            &mut storage,
            ResourceGovernorConfig {
                ram_budget_bytes: 4 * 1024 * 1024 * 1024,
                vram_budget_bytes: 4 * 1024 * 1024 * 1024,
                min_ram_headroom_bytes: 256 * 1024 * 1024,
                min_vram_headroom_bytes: 256 * 1024 * 1024,
                ..ResourceGovernorConfig::default()
            },
        )
        .expect("load governor");

        let plan = governor
            .prepare_activation(&mut storage, &runtime_registry, &session_registry, &target)
            .expect("fit should be admitted");

        assert!(plan.requires_loader_lock);
        assert!(plan.evict_runtime_ids.is_empty());
        assert!(plan.reservation.ram_bytes > 0);
        assert!(plan.reservation.vram_bytes > 0);
    }

    #[test]
    fn admission_can_schedule_lru_eviction_for_idle_runtime() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
        let dir = make_temp_dir("agenticos-resource-governor-evict");
        let db_path = dir.join("agenticos.db");
        let tokenizer_path = write_test_tokenizer(&dir);
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load registry");
        let session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut governor = ResourceGovernor::load(
            &mut storage,
            ResourceGovernorConfig {
                ram_budget_bytes: 3 * 1024 * 1024 * 1024,
                vram_budget_bytes: 3 * 1024 * 1024 * 1024,
                min_ram_headroom_bytes: 256 * 1024 * 1024,
                min_vram_headroom_bytes: 256 * 1024 * 1024,
                local_runtime_ram_scale: 1.0,
                local_runtime_vram_scale: 1.0,
                local_runtime_ram_overhead_bytes: 0,
                local_runtime_vram_overhead_bytes: 0,
                ..ResourceGovernorConfig::default()
            },
        )
        .expect("load governor");

        let left_path = write_model_file(&dir, "left.gguf", 1024 * 1024 * 1024);
        let right_path = write_model_file(&dir, "right.gguf", 2 * 1024 * 1024 * 1024);
        let left_target = local_target(&left_path, &tokenizer_path);
        let right_target = local_target(&right_path, &tokenizer_path);

        let left_reservation = RuntimeReservation {
            ram_bytes: 1024 * 1024 * 1024,
            vram_bytes: 1024 * 1024 * 1024,
        };
        runtime_registry
            .activate_target(&mut storage, &left_target, left_reservation)
            .expect("activate left runtime");

        let plan = governor
            .prepare_activation(
                &mut storage,
                &runtime_registry,
                &session_registry,
                &right_target,
            )
            .expect("eviction plan should be admitted");
        assert_eq!(plan.evict_runtime_ids.len(), 1);
    }

    #[test]
    fn admission_queues_when_only_pinned_runtime_blocks_fit() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
        let dir = make_temp_dir("agenticos-resource-governor-queue");
        let db_path = dir.join("agenticos.db");
        let tokenizer_path = write_test_tokenizer(&dir);
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load registry");
        let session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut governor = ResourceGovernor::load(
            &mut storage,
            ResourceGovernorConfig {
                ram_budget_bytes: 3 * 1024 * 1024 * 1024,
                vram_budget_bytes: 3 * 1024 * 1024 * 1024,
                min_ram_headroom_bytes: 256 * 1024 * 1024,
                min_vram_headroom_bytes: 256 * 1024 * 1024,
                local_runtime_ram_scale: 1.0,
                local_runtime_vram_scale: 1.0,
                local_runtime_ram_overhead_bytes: 0,
                local_runtime_vram_overhead_bytes: 0,
                ..ResourceGovernorConfig::default()
            },
        )
        .expect("load governor");

        let left_path = write_model_file(&dir, "left-pinned.gguf", 1024 * 1024 * 1024);
        let right_path = write_model_file(&dir, "right-queued.gguf", 2 * 1024 * 1024 * 1024);
        let left_target = local_target(&left_path, &tokenizer_path);
        let right_target = local_target(&right_path, &tokenizer_path);

        let left = runtime_registry
            .activate_target(
                &mut storage,
                &left_target,
                RuntimeReservation {
                    ram_bytes: 1024 * 1024 * 1024,
                    vram_bytes: 1024 * 1024 * 1024,
                },
            )
            .expect("activate left runtime");
        runtime_registry
            .set_runtime_pinned(&mut storage, &left.runtime_id, true)
            .expect("pin runtime");

        let result = governor.prepare_activation(
            &mut storage,
            &runtime_registry,
            &session_registry,
            &right_target,
        );
        match result {
            Err(ResourceGovernorError::Busy(message)) => {
                assert!(message.contains("queued"));
            }
            other => panic!("expected queued busy result, got {other:?}"),
        }

        assert_eq!(governor.status(&runtime_registry).pending_queue_depth, 1);
    }

    #[test]
    fn admission_refuses_when_single_runtime_exceeds_budget() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
        let dir = make_temp_dir("agenticos-resource-governor-refuse");
        let db_path = dir.join("agenticos.db");
        let model_path = write_model_file(&dir, "too-big.gguf", 2 * 1024 * 1024 * 1024);
        let tokenizer_path = write_test_tokenizer(&dir);
        let target = local_target(&model_path, &tokenizer_path);
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let runtime_registry = RuntimeRegistry::load(&mut storage).expect("load registry");
        let session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut governor = ResourceGovernor::load(
            &mut storage,
            ResourceGovernorConfig {
                ram_budget_bytes: 1024 * 1024 * 1024,
                vram_budget_bytes: 1024 * 1024 * 1024,
                min_ram_headroom_bytes: 256 * 1024 * 1024,
                min_vram_headroom_bytes: 256 * 1024 * 1024,
                local_runtime_ram_scale: 1.0,
                local_runtime_vram_scale: 1.0,
                local_runtime_ram_overhead_bytes: 0,
                local_runtime_vram_overhead_bytes: 0,
                ..ResourceGovernorConfig::default()
            },
        )
        .expect("load governor");

        let result = governor.prepare_activation(
            &mut storage,
            &runtime_registry,
            &session_registry,
            &target,
        );
        match result {
            Err(ResourceGovernorError::Refused(message)) => {
                assert!(message.contains("refused"));
            }
            other => panic!("expected refusal, got {other:?}"),
        }
    }

    fn local_target(model_path: &PathBuf, tokenizer_path: &PathBuf) -> ResolvedModelTarget {
        let driver =
            resolve_driver_for_model(PromptFamily::Mistral, None, Some("external-llamacpp"))
                .expect("resolve driver");
        ResolvedModelTarget::local(
            Some(
                model_path
                    .file_stem()
                    .expect("file stem")
                    .to_string_lossy()
                    .to_string(),
            ),
            model_path.clone(),
            PromptFamily::Mistral,
            Some(tokenizer_path.clone()),
            None,
            driver,
        )
    }

    fn write_model_file(dir: &Path, name: &str, size_bytes: u64) -> PathBuf {
        let path = dir.join(name);
        let file = std::fs::File::create(&path).expect("create model file");
        file.set_len(size_bytes).expect("size model file");
        path
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time monotonic")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_test_tokenizer(dir: &Path) -> PathBuf {
        let path = dir.join("tokenizer.json");
        let vocab = [
            ("<unk>".to_string(), 0),
            ("hello".to_string(), 1),
            ("</s>".to_string(), 2),
        ]
        .into_iter()
        .collect();
        let model = WordLevel::builder()
            .vocab(vocab)
            .unk_token("<unk>".to_string())
            .build()
            .expect("build tokenizer");
        Tokenizer::new(model)
            .save(&path, false)
            .expect("save tokenizer");
        path
    }
