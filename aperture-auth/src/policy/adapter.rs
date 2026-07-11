//! Casbin adapter backed by turso (libsql) via aperture-storage.

use aperture_storage::PolicyRuleRepository;
use async_trait::async_trait;
use casbin::error::AdapterError;
use casbin::{Adapter, Filter, Model};

/// Casbin adapter that persists policies in the `casbin_rule` table through
/// the storage layer.
pub struct TursoAdapter {
    repo: PolicyRuleRepository,
}

impl TursoAdapter {
    pub(crate) fn new(repo: PolicyRuleRepository) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl Adapter for TursoAdapter {
    async fn load_policy(&mut self, m: &mut dyn Model) -> casbin::Result<()> {
        let rules = self.repo.load_all().await.map_err(map_storage_err)?;
        for rule in rules {
            let sec = rule.ptype.chars().next().unwrap_or('p').to_string();
            m.add_policy(&sec, &rule.ptype, rule.values);
        }
        Ok(())
    }

    async fn load_filtered_policy<'a>(
        &mut self,
        _m: &mut dyn Model,
        _f: Filter<'a>,
    ) -> casbin::Result<()> {
        Ok(())
    }

    async fn save_policy(&mut self, m: &mut dyn Model) -> casbin::Result<()> {
        let mut rules: Vec<(String, Vec<String>)> = Vec::new();
        for (sec, ptype) in [("p", "p"), ("g", "g")] {
            for policy in m.get_policy(sec, ptype) {
                rules.push((ptype.to_owned(), policy));
            }
        }
        self.repo
            .replace_all(&rules)
            .await
            .map_err(map_storage_err)?;
        Ok(())
    }

    async fn clear_policy(&mut self) -> casbin::Result<()> {
        self.repo.clear().await.map_err(map_storage_err)?;
        Ok(())
    }

    fn is_filtered(&self) -> bool {
        false
    }

    async fn add_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        rule: Vec<String>,
    ) -> casbin::Result<bool> {
        self.repo
            .insert(ptype, &rule)
            .await
            .map_err(map_storage_err)?;
        Ok(true)
    }

    async fn add_policies(
        &mut self,
        _sec: &str,
        ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> casbin::Result<bool> {
        let batch: Vec<(String, Vec<String>)> =
            rules.into_iter().map(|r| (ptype.to_owned(), r)).collect();
        self.repo
            .insert_batch(&batch)
            .await
            .map_err(map_storage_err)?;
        Ok(true)
    }

    async fn remove_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        rule: Vec<String>,
    ) -> casbin::Result<bool> {
        let removed = self
            .repo
            .delete(ptype, &rule)
            .await
            .map_err(map_storage_err)?;
        Ok(removed > 0)
    }

    async fn remove_policies(
        &mut self,
        _sec: &str,
        ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> casbin::Result<bool> {
        let batch: Vec<(String, Vec<String>)> =
            rules.into_iter().map(|r| (ptype.to_owned(), r)).collect();
        self.repo
            .delete_batch(&batch)
            .await
            .map_err(map_storage_err)?;
        Ok(true)
    }

    async fn remove_filtered_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        field_index: usize,
        field_values: Vec<String>,
    ) -> casbin::Result<bool> {
        let removed = self
            .repo
            .delete_filtered(ptype, field_index, &field_values)
            .await
            .map_err(map_storage_err)?;
        Ok(removed > 0)
    }
}

/// Converts a storage error into a casbin error.
pub(super) fn map_storage_err(err: aperture_storage::StorageError) -> casbin::Error {
    casbin::Error::AdapterError(AdapterError(Box::new(err)))
}
