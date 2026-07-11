//! Storage for policy rules.
//!
//! The `casbin_rule` table holds casbin's standard `(ptype, v0..v5)` row
//! format. This repository provides the CRUD operations the casbin adapter
//! needs, without coupling the storage layer to casbin itself.

use turso::{Connection, Value, params_from_iter};

use crate::error::{Result, StorageError};
use crate::macros::sql;
use crate::sql::{ToSql, get};

/// One policy or grouping rule, in casbin's flat string format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    /// Policy type: `"p"` for policy rules, `"g"` for grouping (role) rules.
    pub ptype: String,
    /// The rule values (v0 through v5). Unused trailing values are empty
    /// strings.
    pub values: Vec<String>,
}

/// Repository over the casbin_rule table.
pub struct PolicyRuleRepository {
    connection: Connection,
}

impl PolicyRuleRepository {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Loads every rule, ordered by id for deterministic loading.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn load_all(&self) -> Result<Vec<PolicyRule>> {
        let mut rows = self
            .connection
            .query(
                sql!(SELECT ptype, v0, v1, v2, v3, v4, v5 FROM casbin_rule ORDER BY id),
                (),
            )
            .await
            .map_err(StorageError::from_turso)?;
        let mut rules = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::from_turso)? {
            let ptype: String = get(&row, 0)?;
            let mut values = Vec::with_capacity(6);
            for i in 1..=6 {
                values.push(get::<String>(&row, i)?);
            }
            rules.push(PolicyRule { ptype, values });
        }
        Ok(rules)
    }

    /// Inserts a single rule.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn insert(&self, ptype: &str, values: &[String]) -> Result<()> {
        let cols = ["v0", "v1", "v2", "v3", "v4", "v5"];
        let mut params: Vec<Value> = vec![ptype.to_sql()];
        for i in 0..6 {
            if i < values.len() {
                params.push(values[i].to_sql());
            } else {
                params.push("".to_sql());
            }
        }
        self.connection
            .execute(
                sql!(
                    INSERT INTO casbin_rule (ptype, v0, v1, v2, v3, v4, v5)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ),
                params_from_iter(params),
            )
            .await
            .map_err(StorageError::from_turso)?;
        let _ = cols; // silence unused warning if cols not needed elsewhere
        Ok(())
    }

    /// Deletes all rules matching `ptype` and `values` exactly. Returns how
    /// many were removed.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn delete(&self, ptype: &str, values: &[String]) -> Result<usize> {
        let mut params: Vec<Value> = vec![ptype.to_sql()];
        for i in 0..6 {
            if i < values.len() {
                params.push(values[i].to_sql());
            } else {
                params.push("".to_sql());
            }
        }
        let affected = self
            .connection
            .execute(
                sql!(
                    DELETE FROM casbin_rule
                    WHERE ptype = ?1 AND v0 = ?2 AND v1 = ?3 AND v2 = ?4
                        AND v3 = ?5 AND v4 = ?6 AND v5 = ?7
                ),
                params_from_iter(params),
            )
            .await
            .map_err(StorageError::from_turso)?;
        Ok(affected as usize)
    }

    /// Deletes all rules matching `ptype` where the fields starting at
    /// `field_index` equal `field_values`. Fields before `field_index` are
    /// wildcarded. Returns how many were removed.
    #[tracing::instrument(level = "info", skip(self, field_values))]
    pub async fn delete_filtered(
        &self,
        ptype: &str,
        field_index: usize,
        field_values: &[String],
    ) -> Result<usize> {
        let cols = ["v0", "v1", "v2", "v3", "v4", "v5"];
        let mut where_parts: Vec<String> = vec!["ptype = ?1".to_owned()];
        let mut params: Vec<Value> = vec![ptype.to_sql()];
        for (i, value) in field_values.iter().enumerate() {
            let col_idx = field_index + i;
            if col_idx >= cols.len() {
                break;
            }
            let param_idx = i + 2;
            where_parts.push(format!("{} = ?{param_idx}", cols[col_idx]));
            params.push(value.to_sql());
        }
        let where_clause = where_parts.join(" AND ");
        let sql_str = format!("DELETE FROM casbin_rule WHERE {where_clause}");
        let affected = self
            .connection
            .execute(&sql_str, params_from_iter(params))
            .await
            .map_err(StorageError::from_turso)?;
        Ok(affected as usize)
    }

    /// Deletes every rule. Used by `save_policy` to replace all rules.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn clear(&self) -> Result<()> {
        self.connection
            .execute(sql!(DELETE FROM casbin_rule), ())
            .await
            .map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Returns how many rules exist. Used at bootstrap to detect first run.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn count(&self) -> Result<i64> {
        let mut rows = self
            .connection
            .query(sql!(SELECT COUNT(*) FROM casbin_rule), ())
            .await
            .map_err(StorageError::from_turso)?;
        match rows.next().await.map_err(StorageError::from_turso)? {
            Some(row) => Ok(get(&row, 0)?),
            None => Ok(0),
        }
    }
}
