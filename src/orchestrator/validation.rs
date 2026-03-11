use std::collections::{HashMap, HashSet, VecDeque};

use crate::errors::OrchestratorError;

use super::TaskNodeDef;

pub(crate) fn validate_and_sort(tasks: &[TaskNodeDef]) -> Result<Vec<String>, OrchestratorError> {
    if tasks.is_empty() {
        return Err(OrchestratorError::EmptyGraph);
    }

    let mut seen = HashSet::new();
    for task in tasks {
        if !seen.insert(task.id.as_str()) {
            return Err(OrchestratorError::DuplicateTaskId(task.id.clone()));
        }
    }

    let task_ids: HashSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    for task in tasks {
        if task.deps.iter().any(|dep| dep == &task.id) {
            return Err(OrchestratorError::SelfDependency(task.id.clone()));
        }
        for dep in &task.deps {
            if !task_ids.contains(dep.as_str()) {
                return Err(OrchestratorError::UnknownDependency {
                    task: task.id.clone(),
                    dependency: dep.clone(),
                });
            }
        }
    }

    topological_sort(tasks)
}

pub(crate) fn topological_sort(tasks: &[TaskNodeDef]) -> Result<Vec<String>, OrchestratorError> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

    for task in tasks {
        in_degree.entry(task.id.as_str()).or_insert(0);
        adjacency.entry(task.id.as_str()).or_default();
        for dep in &task.deps {
            adjacency
                .entry(dep.as_str())
                .or_default()
                .push(task.id.as_str());
            *in_degree.entry(task.id.as_str()).or_insert(0) += 1;
        }
    }

    let mut roots: Vec<&str> = in_degree
        .iter()
        .filter_map(|(&id, &degree)| if degree == 0 { Some(id) } else { None })
        .collect();
    roots.sort_unstable();

    let mut queue: VecDeque<&str> = roots.into_iter().collect();
    let mut result = Vec::new();

    while let Some(node) = queue.pop_front() {
        result.push(node.to_string());
        if let Some(children) = adjacency.get(node) {
            let mut ready = Vec::new();
            for &child in children {
                if let Some(degree) = in_degree.get_mut(child) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        ready.push(child);
                    }
                }
            }
            ready.sort_unstable();
            queue.extend(ready);
        }
    }

    if result.len() != tasks.len() {
        return Err(OrchestratorError::CycleDetected);
    }

    Ok(result)
}
