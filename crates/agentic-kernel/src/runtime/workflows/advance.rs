use crate::orchestrator::{Orchestrator, SpawnRequest};

pub(super) fn collect_orchestrator_actions(
    orchestrator: &mut Orchestrator,
) -> (Vec<SpawnRequest>, Vec<u64>) {
    orchestrator.advance()
}
