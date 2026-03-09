use std::collections::HashMap;

use crate::model_catalog::WorkloadClass;
use crate::policy::workload_from_label_or_default;

use super::output::build_task_prompt;
use super::validation::topological_sort;
use super::*;

fn make_linear_graph() -> TaskGraphDef {
    TaskGraphDef {
        tasks: vec![
            TaskNodeDef {
                id: "A".into(),
                prompt: "Task A".into(),
                workload: None,
                deps: vec![],
            },
            TaskNodeDef {
                id: "B".into(),
                prompt: "Task B".into(),
                workload: Some("code".into()),
                deps: vec!["A".into()],
            },
            TaskNodeDef {
                id: "C".into(),
                prompt: "Task C".into(),
                workload: None,
                deps: vec!["B".into()],
            },
        ],
        failure_policy: FailurePolicy::FailFast,
    }
}

fn make_parallel_graph() -> TaskGraphDef {
    TaskGraphDef {
        tasks: vec![
            TaskNodeDef {
                id: "A".into(),
                prompt: "Task A".into(),
                workload: None,
                deps: vec![],
            },
            TaskNodeDef {
                id: "B".into(),
                prompt: "Task B".into(),
                workload: None,
                deps: vec!["A".into()],
            },
            TaskNodeDef {
                id: "C".into(),
                prompt: "Task C".into(),
                workload: None,
                deps: vec!["A".into()],
            },
            TaskNodeDef {
                id: "D".into(),
                prompt: "Task D".into(),
                workload: None,
                deps: vec!["B".into(), "C".into()],
            },
        ],
        failure_policy: FailurePolicy::BestEffort,
    }
}

#[test]
fn linear_graph_registers_and_spawns_root() {
    let mut orch = Orchestrator::new();
    let (id, spawns) = orch.register(make_linear_graph(), 1).expect("register");
    assert_eq!(id, 1);
    assert_eq!(spawns.len(), 1);
    assert_eq!(spawns[0].task_id, "A");
    assert_eq!(spawns[0].prompt, "Task A");
}

#[test]
fn parallel_graph_spawns_single_root() {
    let mut orch = Orchestrator::new();
    let (_, spawns) = orch.register(make_parallel_graph(), 1).unwrap();
    assert_eq!(spawns.len(), 1);
    assert_eq!(spawns[0].task_id, "A");
}

#[test]
fn linear_graph_advances_step_by_step() {
    let mut orch = Orchestrator::new();
    let (id, spawns) = orch.register(make_linear_graph(), 1).unwrap();

    let pid_a = 100;
    orch.register_pid(pid_a, id, &spawns[0].task_id);
    orch.append_output(pid_a, "result of A");
    orch.mark_completed(pid_a);

    let (ready, kills) = orch.advance();
    assert!(kills.is_empty());
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].task_id, "B");
    assert!(ready[0].prompt.contains("result of A"));
    assert!(ready[0].prompt.contains("Task B"));

    let pid_b = 101;
    orch.register_pid(pid_b, id, &ready[0].task_id);
    orch.append_output(pid_b, "result of B");
    orch.mark_completed(pid_b);

    let (ready, _) = orch.advance();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].task_id, "C");
    assert!(ready[0].prompt.contains("result of B"));

    let pid_c = 102;
    orch.register_pid(pid_c, id, &ready[0].task_id);
    orch.mark_completed(pid_c);

    let (ready, _) = orch.advance();
    assert!(ready.is_empty());
    assert!(orch.get(id).unwrap().is_finished());
}

#[test]
fn parallel_graph_spawns_b_and_c_after_a() {
    let mut orch = Orchestrator::new();
    let (id, _) = orch.register(make_parallel_graph(), 1).unwrap();

    let pid_a = 100;
    orch.register_pid(pid_a, id, "A");
    orch.append_output(pid_a, "A output");
    orch.mark_completed(pid_a);

    let (ready, _) = orch.advance();
    assert_eq!(ready.len(), 2);
    let ids: Vec<&str> = ready.iter().map(|req| req.task_id.as_str()).collect();
    assert!(ids.contains(&"B"));
    assert!(ids.contains(&"C"));

    let pid_b = 101;
    let pid_c = 102;
    orch.register_pid(pid_b, id, "B");
    orch.register_pid(pid_c, id, "C");
    orch.append_output(pid_b, "B output");
    orch.append_output(pid_c, "C output");
    orch.mark_completed(pid_b);
    orch.mark_completed(pid_c);

    let (ready, _) = orch.advance();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].task_id, "D");
    assert!(ready[0].prompt.contains("B output"));
    assert!(ready[0].prompt.contains("C output"));
}

#[test]
fn fail_fast_skips_pending_on_failure() {
    let mut orch = Orchestrator::new();
    let (id, spawns) = orch.register(make_linear_graph(), 1).unwrap();

    let pid_a = 100;
    orch.register_pid(pid_a, id, &spawns[0].task_id);
    orch.mark_failed(pid_a, "process error");

    let (ready, kill_pids) = orch.advance();
    assert!(ready.is_empty());
    assert!(kill_pids.is_empty());

    let orch_state = orch.get(id).unwrap();
    assert!(matches!(orch_state.status["B"], TaskStatus::Skipped));
    assert!(matches!(orch_state.status["C"], TaskStatus::Skipped));
    assert!(orch_state.is_finished());
}

#[test]
fn fail_fast_kills_running_tasks() {
    let mut orch = Orchestrator::new();
    let graph = TaskGraphDef {
        tasks: vec![
            TaskNodeDef { id: "A".into(), prompt: "A".into(), workload: None, deps: vec![] },
            TaskNodeDef { id: "B".into(), prompt: "B".into(), workload: None, deps: vec!["A".into()] },
            TaskNodeDef { id: "C".into(), prompt: "C".into(), workload: None, deps: vec!["A".into()] },
        ],
        failure_policy: FailurePolicy::FailFast,
    };
    let (id, _) = orch.register(graph, 1).unwrap();

    let pid_a = 100;
    orch.register_pid(pid_a, id, "A");
    orch.mark_completed(pid_a);
    let (ready, _) = orch.advance();
    assert_eq!(ready.len(), 2);

    let pid_b = 101;
    let pid_c = 102;
    orch.register_pid(pid_b, id, "B");
    orch.register_pid(pid_c, id, "C");
    orch.mark_failed(pid_b, "oops");

    let (ready, kill_pids) = orch.advance();
    assert!(ready.is_empty());
    assert!(kill_pids.contains(&pid_c));
    assert!(orch.get(id).unwrap().is_finished());
}

#[test]
fn best_effort_skips_dependents_of_failed() {
    let mut orch = Orchestrator::new();
    let (id, _) = orch.register(make_parallel_graph(), 1).unwrap();

    let pid_a = 100;
    orch.register_pid(pid_a, id, "A");
    orch.append_output(pid_a, "A done");
    orch.mark_completed(pid_a);

    let (ready, _) = orch.advance();
    assert_eq!(ready.len(), 2);

    let pid_b = 101;
    let pid_c = 102;
    orch.register_pid(pid_b, id, "B");
    orch.register_pid(pid_c, id, "C");
    orch.mark_failed(pid_b, "B error");
    orch.append_output(pid_c, "C output");
    orch.mark_completed(pid_c);

    let (ready, kill_pids) = orch.advance();
    assert!(kill_pids.is_empty());
    assert!(ready.is_empty());
    assert!(matches!(orch.get(id).unwrap().status["D"], TaskStatus::Skipped));
    assert!(orch.get(id).unwrap().is_finished());
}

#[test]
fn cyclic_graph_rejected() {
    let graph = TaskGraphDef {
        tasks: vec![
            TaskNodeDef { id: "A".into(), prompt: "A".into(), workload: None, deps: vec!["B".into()] },
            TaskNodeDef { id: "B".into(), prompt: "B".into(), workload: None, deps: vec!["A".into()] },
        ],
        failure_policy: FailurePolicy::FailFast,
    };
    let mut orch = Orchestrator::new();
    let err = orch.register(graph, 1).expect_err("cycle should fail");
    assert!(err.to_string().contains("cycle"));
}

#[test]
fn empty_graph_rejected() {
    let graph = TaskGraphDef {
        tasks: vec![],
        failure_policy: FailurePolicy::FailFast,
    };
    let mut orch = Orchestrator::new();
    let err = orch.register(graph, 1).expect_err("empty should fail");
    assert!(err.to_string().contains("empty"));
}

#[test]
fn duplicate_task_id_rejected() {
    let graph = TaskGraphDef {
        tasks: vec![
            TaskNodeDef { id: "A".into(), prompt: "A".into(), workload: None, deps: vec![] },
            TaskNodeDef { id: "A".into(), prompt: "A2".into(), workload: None, deps: vec![] },
        ],
        failure_policy: FailurePolicy::FailFast,
    };
    let mut orch = Orchestrator::new();
    let err = orch.register(graph, 1).expect_err("duplicate should fail");
    assert!(err.to_string().contains("duplicate"));
}

#[test]
fn unknown_dependency_rejected() {
    let graph = TaskGraphDef {
        tasks: vec![TaskNodeDef {
            id: "A".into(),
            prompt: "A".into(),
            workload: None,
            deps: vec!["Z".into()],
        }],
        failure_policy: FailurePolicy::FailFast,
    };
    let mut orch = Orchestrator::new();
    let err = orch.register(graph, 1).expect_err("unknown dep should fail");
    assert!(err.to_string().contains("unknown task"));
}

#[test]
fn self_dependency_rejected() {
    let graph = TaskGraphDef {
        tasks: vec![TaskNodeDef {
            id: "A".into(),
            prompt: "A".into(),
            workload: None,
            deps: vec!["A".into()],
        }],
        failure_policy: FailurePolicy::FailFast,
    };
    let mut orch = Orchestrator::new();
    let err = orch.register(graph, 1).expect_err("self-dep should fail");
    assert!(err.to_string().contains("depends on itself"));
}

#[test]
fn topological_sort_deterministic() {
    let tasks = vec![
        TaskNodeDef { id: "C".into(), prompt: String::new(), workload: None, deps: vec!["A".into()] },
        TaskNodeDef { id: "A".into(), prompt: String::new(), workload: None, deps: vec![] },
        TaskNodeDef { id: "B".into(), prompt: String::new(), workload: None, deps: vec!["A".into()] },
    ];
    let order = topological_sort(&tasks).unwrap();
    assert_eq!(order, vec!["A", "B", "C"]);
}

#[test]
fn format_status_includes_all_info() {
    let mut orch = Orchestrator::new();
    let (id, _) = orch.register(make_linear_graph(), 1).unwrap();
    let status = orch.format_status(id).expect("status should exist");
    assert!(status.contains("orchestration_id=1"));
    assert!(status.contains("total=3"));
    assert!(status.contains("task=A"));
    assert!(status.contains("task=B"));
    assert!(status.contains("task=C"));
}

#[test]
fn json_deserialization() {
    let json = r#"{
            "tasks": [
                {"id": "step1", "prompt": "Hello", "workload": "fast", "deps": []},
                {"id": "step2", "prompt": "World", "deps": ["step1"]}
            ],
            "failure_policy": "best_effort"
        }"#;
    let graph: TaskGraphDef = serde_json::from_str(json).expect("parse");
    assert_eq!(graph.tasks.len(), 2);
    assert_eq!(graph.failure_policy, FailurePolicy::BestEffort);
    assert_eq!(graph.tasks[0].workload.as_deref(), Some("fast"));
    assert!(graph.tasks[1].workload.is_none());
}

#[test]
fn json_default_policy_is_fail_fast() {
    let json = r#"{"tasks": [{"id": "a", "prompt": "hi"}]}"#;
    let graph: TaskGraphDef = serde_json::from_str(json).expect("parse");
    assert_eq!(graph.failure_policy, FailurePolicy::FailFast);
}

#[test]
fn workload_parsing() {
    assert!(matches!(workload_from_label_or_default(Some("fast")), WorkloadClass::Fast));
    assert!(matches!(workload_from_label_or_default(Some("CODE")), WorkloadClass::Code));
    assert!(matches!(workload_from_label_or_default(Some("reasoning")), WorkloadClass::Reasoning));
    assert!(matches!(workload_from_label_or_default(None), WorkloadClass::General));
    assert!(matches!(workload_from_label_or_default(Some("unknown")), WorkloadClass::General));
}

#[test]
fn build_prompt_injects_dependency_output() {
    let task = TaskNodeDef {
        id: "D".into(),
        prompt: "Summarise everything".into(),
        workload: None,
        deps: vec!["A".into(), "B".into()],
    };
    let mut outputs = HashMap::new();
    outputs.insert("A".to_string(), "output A".to_string());
    outputs.insert("B".to_string(), "output B".to_string());

    let prompt = build_task_prompt(&task, &outputs);
    assert!(prompt.contains("output A"));
    assert!(prompt.contains("output B"));
    assert!(prompt.contains("Summarise everything"));
}

#[test]
fn build_prompt_without_deps_returns_raw() {
    let task = TaskNodeDef {
        id: "root".into(),
        prompt: "do it".into(),
        workload: None,
        deps: vec![],
    };
    let prompt = build_task_prompt(&task, &HashMap::new());
    assert_eq!(prompt, "do it");
}

#[test]
fn append_output_truncates_and_marks_status() {
    let mut orch = Orchestrator::new();
    orch.max_output_chars = 24;

    let (id, spawns) = orch.register(make_linear_graph(), 1).unwrap();
    let pid_a = 100;
    orch.register_pid(pid_a, id, &spawns[0].task_id);
    orch.append_output(pid_a, "abcdefghijklmnopqrstuvwxyz");

    let stored = orch.get(id).unwrap().output.get("A").cloned().unwrap_or_default();
    assert!(stored.contains("[TRUNCATED]"));
    assert!(orch.get(id).unwrap().truncated_outputs >= 1);
    assert!(orch.get(id).unwrap().output_chars_stored <= 24);
}