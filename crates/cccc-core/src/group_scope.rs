use cccc_contracts::utc_now;
use std::io;

use crate::{GroupDoc, GroupStore, Registry, Scope};

pub fn attach(store: &GroupStore, group_id: &str, scope: Scope) -> io::Result<GroupDoc> {
    let result = store.mutate(group_id, |group| {
        if let Some(existing) = group
            .scopes
            .iter_mut()
            .find(|item| item.scope_key == scope.scope_key)
        {
            existing.clone_from(&scope);
        } else {
            group.scopes.push(scope.clone());
        }
        group.active_scope_key.clone_from(&scope.scope_key);
        Ok(group.clone())
    })?;
    Registry::mutate(store.home(), |registry| {
        registry
            .defaults
            .insert(scope.scope_key.clone(), group_id.into());
        if let Some(meta) = registry.groups.get_mut(group_id) {
            meta.default_scope_key.clone_from(&scope.scope_key);
            meta.updated_at = utc_now();
        }
        Ok(())
    })?;
    Ok(result)
}

pub fn detach(store: &GroupStore, group_id: &str, scope_key: &str) -> io::Result<GroupDoc> {
    let result = store.mutate(group_id, |group| {
        let before = group.scopes.len();
        group.scopes.retain(|scope| scope.scope_key != scope_key);
        if before == group.scopes.len() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "scope not attached",
            ));
        }
        if group.active_scope_key == scope_key {
            group.active_scope_key = group
                .scopes
                .first()
                .map(|scope| scope.scope_key.clone())
                .unwrap_or_default();
        }
        Ok(group.clone())
    })?;
    Registry::mutate(store.home(), |registry| {
        registry.defaults.remove(scope_key);
        if let Some(meta) = registry.groups.get_mut(group_id) {
            meta.default_scope_key.clone_from(&result.active_scope_key);
        }
        Ok(())
    })?;
    Ok(result)
}

pub fn activate(store: &GroupStore, group_id: &str, scope_key: &str) -> io::Result<GroupDoc> {
    let result = store.mutate(group_id, |group| {
        if !group
            .scopes
            .iter()
            .any(|scope| scope.scope_key == scope_key)
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "scope not attached",
            ));
        }
        group.active_scope_key = scope_key.into();
        Ok(group.clone())
    })?;
    Registry::mutate(store.home(), |registry| {
        registry.defaults.insert(scope_key.into(), group_id.into());
        Ok(())
    })?;
    Ok(result)
}
