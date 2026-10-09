use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

type Order = (String, String);

#[derive(Default)]
struct GroupIndex {
    order: BTreeSet<Order>,
    unresolved: BTreeSet<Order>,
    busy: BTreeSet<Order>,
    queued: BTreeSet<Order>,
    unprocessed_document_sources: usize,
}

impl GroupIndex {
    fn update(&mut self, task: &SecretaryTask, insert: bool) {
        let order = (task.created_at.clone(), task.task_id.clone());
        for (set, included) in [
            (&mut self.order, true),
            (&mut self.unresolved, task.unresolved()),
            (
                &mut self.busy,
                task.phase.executing() || !task.cleanup_confirmed,
            ),
            (&mut self.queued, task.phase == SecretaryTaskPhase::Queued),
        ] {
            if insert && included {
                set.insert(order.clone());
            } else {
                set.remove(&order);
            }
        }
        if task.target.kind == cccc_contracts::voice_secretary::SecretaryTaskKind::Document
            && task.superseded_by.is_empty()
            && task.phase != SecretaryTaskPhase::Done
        {
            if insert {
                self.unprocessed_document_sources += task.inputs.len();
            } else {
                self.unprocessed_document_sources -= task.inputs.len();
            }
        }
    }
}

#[derive(Default)]
pub(super) struct TaskIndex {
    tasks: HashMap<String, SecretaryTask>,
    order: BTreeSet<Order>,
    groups: BTreeMap<String, GroupIndex>,
    queued: BTreeSet<Order>,
    unfinished: BTreeSet<Order>,
    sources: HashMap<(String, String), String>,
    successors: HashMap<String, String>,
}

impl TaskIndex {
    pub(super) fn insert(&mut self, task: SecretaryTask) {
        if let Some(previous) = self.tasks.get(&task.task_id) {
            self.queued.remove(&queued_order(previous));
            self.groups
                .entry(previous.target.group_id.clone())
                .or_default()
                .update(previous, false);
        }
        let order = (task.created_at.clone(), task.task_id.clone());
        self.order.insert(order.clone());
        self.groups
            .entry(task.target.group_id.clone())
            .or_default()
            .update(&task, true);
        if task.phase == SecretaryTaskPhase::Queued {
            self.queued.insert(queued_order(&task));
        }
        if !task.cleanup_confirmed
            || task.phase != SecretaryTaskPhase::Done && task.superseded_by.is_empty()
        {
            self.unfinished.insert(order.clone());
        } else {
            self.unfinished.remove(&order);
        }
        for input in &task.inputs {
            if let Some(id) = input["input_id"].as_str().filter(|id| !id.is_empty()) {
                self.sources
                    .entry((task.target.group_id.clone(), id.into()))
                    .or_insert_with(|| task.task_id.clone());
            }
        }
        if !task.previous_task_id.is_empty() {
            self.successors
                .insert(task.previous_task_id.clone(), task.task_id.clone());
        }
        self.tasks.insert(task.task_id.clone(), task);
    }

    pub(super) fn load(&self, id: &str) -> io::Result<SecretaryTask> {
        self.tasks
            .get(id)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Secretary task not found"))
    }

    pub(super) fn list(&self, group: Option<&str>) -> Vec<SecretaryTask> {
        let order = match group {
            Some(group) => match self.groups.get(group) {
                Some(group) => &group.order,
                None => return Vec::new(),
            },
            None => &self.order,
        };
        self.collect(order)
    }

    pub(super) fn group_view(&self, group: &str, recent: usize) -> SecretaryGroupView {
        let Some(group) = self.groups.get(group) else {
            return SecretaryGroupView::default();
        };
        let busy = !group.busy.is_empty();
        let latest = group
            .unresolved
            .last()
            .and_then(|(_, id)| self.tasks.get(id));
        let status = if busy {
            Some(SecretaryTaskPhase::Running)
        } else if !group.queued.is_empty() {
            Some(SecretaryTaskPhase::Queued)
        } else {
            latest.map(|task| task.phase)
        };
        let selected = group.unresolved.iter().rev().chain(
            group
                .order
                .iter()
                .rev()
                .filter(|order| !group.unresolved.contains(*order))
                .take(recent),
        );
        SecretaryGroupView {
            tasks: selected
                .filter_map(|(_, id)| self.tasks.get(id).cloned())
                .collect(),
            busy,
            pending: !group.unresolved.is_empty(),
            status,
            unprocessed_document_sources: group.unprocessed_document_sources,
        }
    }

    fn collect(&self, order: &BTreeSet<Order>) -> Vec<SecretaryTask> {
        order
            .iter()
            .filter_map(|(_, id)| self.tasks.get(id).cloned())
            .collect()
    }

    pub(super) fn queued(&self) -> Vec<SecretaryTask> {
        self.collect(&self.queued)
    }
    pub(super) fn unfinished(&self) -> Vec<SecretaryTask> {
        self.collect(&self.unfinished)
    }

    pub(super) fn source_task(&self, group: &str, input: &str) -> Option<String> {
        self.sources.get(&(group.into(), input.into())).cloned()
    }

    pub(super) fn successor(&self, id: &str) -> Option<SecretaryTask> {
        self.successors
            .get(id)
            .and_then(|next| self.tasks.get(next))
            .cloned()
    }

    pub(super) fn remove(&mut self, id: &str) {
        if let Some(task) = self.tasks.remove(id) {
            self.queued.remove(&queued_order(&task));
            let order = (task.created_at.clone(), task.task_id.clone());
            self.order.remove(&order);
            self.unfinished.remove(&order);
            if let Some(group) = self.groups.get_mut(&task.target.group_id) {
                group.update(&task, false);
            }
            self.sources.retain(|_, value| value != id);
            self.successors
                .retain(|key, value| key != id && value != id);
        }
    }
}
