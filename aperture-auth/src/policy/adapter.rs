//! Casbin adapter backed by turso (libsql) via aperture-storage.

use std::str::FromStr;

use aperture_storage::{PolicyRuleRepository, PolicyType};
use async_trait::async_trait;
use casbin::error::AdapterError;
use casbin::{Adapter, Filter, Model};

/// Casbin adapter that persists policies in the `casbin_rule` table through
/// the storage layer.
pub struct TursoAdapter {
    repo: PolicyRuleRepository,
}

impl TursoAdapter {
    pub(crate) const fn new(repo: PolicyRuleRepository) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl Adapter for TursoAdapter {
    async fn load_policy(&mut self, m: &mut dyn Model) -> casbin::Result<()> {
        let rules = self.repo.load_all().await.map_err(map_storage_err)?;
        for rule in rules {
            let ptype = rule.ptype.as_db();
            m.add_policy(ptype, ptype, leading_values(rule.values));
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
        let mut rules: Vec<(PolicyType, Vec<String>)> = Vec::new();
        for (sec, ptype) in [("p", "p"), ("g", "g")] {
            let ty = PolicyType::from_str(ptype).map_err(map_storage_err)?;
            for policy in m.get_policy(sec, ptype) {
                rules.push((ty, policy));
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
        let ptype = PolicyType::from_str(ptype).map_err(map_storage_err)?;
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
        let ptype = PolicyType::from_str(ptype).map_err(map_storage_err)?;
        let batch: Vec<(PolicyType, Vec<String>)> = rules.into_iter().map(|r| (ptype, r)).collect();
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
        let ptype = PolicyType::from_str(ptype).map_err(map_storage_err)?;
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
        let ptype = PolicyType::from_str(ptype).map_err(map_storage_err)?;
        let batch: Vec<(PolicyType, Vec<String>)> = rules.into_iter().map(|r| (ptype, r)).collect();
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
        let ptype = PolicyType::from_str(ptype).map_err(map_storage_err)?;
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

/// Takes the leading present values as the rule.
///
/// Rows are always written with only trailing NULLs, so the leading `Some`
/// prefix is the rule. A legitimate empty-string token survives as
/// `Some("")` and is never dropped.
fn leading_values(values: Vec<Option<String>>) -> Vec<String> {
    values
        .into_iter()
        .take_while(Option::is_some)
        .map(Option::unwrap)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::leading_values;

    #[test]
    fn takes_only_leading_present_values() {
        let values = |parts: &[Option<&str>]| {
            parts
                .iter()
                .map(|part| part.map(str::to_owned))
                .collect::<Vec<_>>()
        };
        assert_eq!(leading_values(vec![]), Vec::<String>::new());
        assert_eq!(
            leading_values(values(&[Some("a"), Some("b"), None, None])),
            vec!["a".to_owned(), "b".to_owned()]
        );
        // An empty-string token is a real value, not absence.
        assert_eq!(
            leading_values(values(&[Some("a"), Some(""), None])),
            vec!["a".to_owned(), String::new()]
        );
        // A Some after a None cannot occur through the insert paths, but the
        // leading prefix still wins.
        assert_eq!(
            leading_values(values(&[Some("a"), None, Some("b")])),
            vec!["a".to_owned()]
        );
    }
}
