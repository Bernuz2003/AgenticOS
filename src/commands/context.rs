use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::prompting::PromptFamily;
use crate::scheduler::ProcessScheduler;
use crate::transport::Client;

/// Shared references for all command handlers, replacing 11 loose parameters.
pub(crate) struct CommandContext<'a> {
    pub client: &'a mut Client,
    pub memory: &'a Rc<RefCell<NeuralMemory>>,
    pub engine_state: &'a Arc<Mutex<Option<LLMEngine>>>,
    pub model_catalog: &'a mut ModelCatalog,
    pub active_family: &'a mut PromptFamily,
    pub scheduler: &'a mut ProcessScheduler,
    pub orchestrator: &'a mut Orchestrator,
    pub client_id: usize,
    pub shutdown_requested: &'a Arc<AtomicBool>,
}
