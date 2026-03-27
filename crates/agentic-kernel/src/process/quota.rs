#![allow(dead_code)]

use super::ContextPolicy;

pub(crate) fn context_window_tokens(policy: &ContextPolicy) -> usize {
    policy.window_size_tokens
}
