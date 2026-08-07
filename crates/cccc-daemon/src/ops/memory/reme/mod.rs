mod common;
mod context;
mod daily;
mod search;
mod write;

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;

use crate::dispatch::OpResult;

pub(super) fn reme_search(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    search::reme_search(home, request)
}

pub(super) fn reme_get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    search::reme_get(home, request)
}

pub(super) fn context_check(request: &DaemonRequest) -> OpResult {
    context::context_check(request)
}

pub(super) fn compact(request: &DaemonRequest) -> OpResult {
    context::compact(request)
}

pub(super) fn daily_flush(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    daily::daily_flush(home, request)
}

pub(super) fn reme_write(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    write::reme_write(home, request)
}
