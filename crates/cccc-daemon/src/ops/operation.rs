use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;

use crate::dispatch::OpResult;

/// Declared beside the handler. Resolving an operation must not perform I/O or
/// mutate state: the dispatcher uses its policy before acquiring a permit.
pub(crate) struct Operation {
    pub(crate) policy: Policy,
    handler: fn(&HomeLayout, &DaemonRequest) -> OpResult,
}

#[derive(Clone, Copy)]
pub(crate) enum Policy {
    /// Shared access to the requested Group, or the global scope without a Group.
    Read,
    /// Exclusive access to the requested Group, or the global scope without one.
    Write,
    GlobalWrite,
    /// A resource owns synchronization, but new work participates in shutdown.
    GlobalRead,
    /// Completion/read/input paths that must remain available during lifecycle
    /// draining. Authorization and resource synchronization remain in the handler.
    ResourceOwned,
}

impl Operation {
    pub(crate) fn new(
        policy: Policy,
        handler: fn(&HomeLayout, &DaemonRequest) -> OpResult,
    ) -> Self {
        Self { policy, handler }
    }

    pub(crate) fn execute(&self, home: &HomeLayout, request: &DaemonRequest) -> OpResult {
        (self.handler)(home, request)
    }
}
