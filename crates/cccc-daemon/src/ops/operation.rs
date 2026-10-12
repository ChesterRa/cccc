use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;

use crate::dispatch::{OpError, OpResult};

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
    /// Membership and remote-access ownership, without blocking Group work.
    RemoteAccess,
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
        if let Some(binding) = request.args.get("_cccc_web_binding") {
            let stage = binding["dots_stage"].as_str();
            let allowed = match stage {
                Some("claim") => request.op == "dots_work_claim",
                Some("completion") => request.op == "runtime_complete_turn",
                _ => true,
            };
            // The permit is already held here. Recheck captured cloud authority
            // after any Stop, configuration change or preceding cloud claim.
            if !allowed
                || cccc_core::web_model_connectors::validate_binding(home, binding).is_err()
                || request.args.get("group_id") != binding.get("group_id")
                || request.args.get("by") != binding.get("actor_id")
            {
                return Err(OpError::new(
                    "permission_denied",
                    "The captured Web Model authority is no longer valid",
                ));
            }
        }
        (self.handler)(home, request)
    }
}
