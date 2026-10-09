use super::*;

impl Manager {
    pub(super) fn source_key(group_id: &str, input: &Value) -> io::Result<(String, u64, String)> {
        let id = input["input_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Secretary source ID is missing")
            })?;
        Ok((
            group_id.into(),
            input["seq"].as_u64().unwrap_or(0),
            id.into(),
        ))
    }

    pub(super) fn accept(&self, group_id: &str, input: &Value) -> io::Result<Option<String>> {
        let manager = self;
        let home = &self.home;
        let key = Self::source_key(group_id, input)?;
        if let Some(id) = self.store.source_task(group_id, &key.2)? {
            self.pending_sources
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
            return Ok(Some(id));
        }
        // Capture and task execution are separate outcomes. Unconfigured or
        // saturated execution keeps the canonical source without allocating a job.
        self.pending_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.clone(), input.clone());
        if configuration::held_baselines(home)?
            .get(group_id)
            .is_some_and(|seq| key.1 <= *seq)
        {
            return Ok(None);
        }
        if resolve_runtime(home).is_err() {
            return Ok(None);
        }
        let tasks = manager.store.queued()?;
        let target = match source_target(home, group_id, input) {
            Ok(target) => target,
            Err(error) => {
                if error.kind() == io::ErrorKind::InvalidData {
                    self.source_errors
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(key);
                }
                return Err(error);
            }
        };
        let guidance = input["secretary_guidance"]
            .as_str()
            .map(str::to_owned)
            .map(Ok)
            .unwrap_or_else(|| super::super::assistants::secretary_guidance(home, group_id))?;
        if target.kind == SecretaryTaskKind::Document && target.request_id.is_empty() {
            if let Some(task) = tasks.iter().rev().find(|task| {
                task.target == target
                    && task.guidance == guidance
                    && task.phase == SecretaryTaskPhase::Queued
                    && task.inputs.len() < 32
                    && input_bytes(&task.inputs) + input["text"].as_str().unwrap_or("").len()
                        <= MAX_BATCH_BYTES
            }) {
                let joined = manager.store.update(&task.task_id, |task| {
                    if task.phase != SecretaryTaskPhase::Queued {
                        return Ok(false);
                    }
                    task.inputs.push(input.clone());
                    Ok(true)
                })?;
                if joined {
                    self.pending_sources
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&key);
                    manager.notify();
                    return Ok(Some(task.task_id.clone()));
                }
            }
        }
        if tasks
            .iter()
            .filter(|t| t.phase == SecretaryTaskPhase::Queued)
            .count()
            >= MAX_QUEUED_TASKS
        {
            return Ok(None);
        }
        let mut task = SecretaryTask::new(target, vec![input.clone()]);
        task.guidance = guidance;
        manager.store.create(&task)?;
        self.pending_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&key);
        manager.notify();
        Ok(Some(task.task_id))
    }

    pub(super) fn load_sources(&self) -> io::Result<()> {
        let _acceptance = self.acceptance.lock().unwrap_or_else(|e| e.into_inner());
        let groups = cccc_core::GroupStore::new(self.home.clone())?.list()?;
        for group in groups {
            for input in
                super::super::assistants::secretary_global_inputs(&self.home, &group.group_id)?
            {
                let key = Self::source_key(&group.group_id, &input).unwrap_or_else(|_| {
                    (
                        group.group_id.clone(),
                        input["seq"].as_u64().unwrap_or(0),
                        format!("invalid-{}", digest(input.to_string().as_bytes())),
                    )
                });
                if self.store.source_task(&group.group_id, &key.2)?.is_none() {
                    if Self::source_key(&group.group_id, &input).is_err()
                        || source_target(&self.home, &group.group_id, &input)
                            .is_err_and(|error| error.kind() == io::ErrorKind::InvalidData)
                    {
                        self.source_errors
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(key.clone());
                    }
                    self.pending_sources
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(key, input);
                }
            }
        }
        Ok(())
    }

    pub(super) fn reconcile_sources(&self) -> io::Result<()> {
        if resolve_runtime(&self.home).is_err() {
            return Ok(());
        }
        let _acceptance = self.acceptance.lock().unwrap_or_else(|e| e.into_inner());
        let held = configuration::held_baselines(&self.home)?;
        let pending = self
            .pending_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(key, input)| (key.clone(), input.clone()))
            .collect::<Vec<_>>();
        for (key, input) in pending {
            if self
                .source_errors
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&key)
                || held.get(&key.0).is_some_and(|seq| key.1 <= *seq)
            {
                continue;
            }
            if self.store.queued()?.len() >= MAX_QUEUED_TASKS {
                break;
            }
            match self.accept(&key.0, &input) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                    // Keep the canonical source intact. One invalid target must
                    // not block later inputs or other Groups on every wake.
                    self.source_errors
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(key);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn source_target(
    home: &HomeLayout,
    group_id: &str,
    input: &Value,
) -> io::Result<cccc_contracts::voice_secretary::SecretaryTaskTarget> {
    let target: cccc_contracts::voice_secretary::SecretaryTaskTarget =
        if input["secretary_target"].is_object() {
            serde_json::from_value(input["secretary_target"].clone()).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Secretary source target is invalid; its original input is retained",
                )
            })?
        } else {
            super::super::assistants::secretary_target(home, group_id, input)?
        };
    if target.group_id != group_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Secretary source target belongs to another Group",
        ));
    }
    Ok(target)
}

#[derive(Default)]
pub(super) struct SourceCounts {
    pub deferred: usize,
    pub held: usize,
    pub invalid: usize,
}

impl Manager {
    pub(super) fn source_counts(
        &self,
        group: Option<&str>,
        held: &BTreeMap<String, u64>,
    ) -> SourceCounts {
        let errors = self.source_errors.lock().unwrap_or_else(|e| e.into_inner());
        let pending = self
            .pending_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut counts = SourceCounts::default();
        for key in pending
            .keys()
            .filter(|key| group.is_none_or(|group| group == key.0))
        {
            if errors.contains(key) {
                counts.invalid += 1;
            } else if held.get(&key.0).is_some_and(|seq| key.1 <= *seq) {
                counts.held += 1;
            } else {
                counts.deferred += 1;
            }
        }
        counts
    }

    pub(super) fn hold_sources(&self, held: &mut BTreeMap<String, u64>) {
        for (group, seq, _) in self
            .pending_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
        {
            held.entry(group.clone())
                .and_modify(|old| *old = (*old).max(*seq))
                .or_insert(*seq);
        }
    }
}
