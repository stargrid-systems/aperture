//! Storage for policy rules.
//!
//! The `casbin_rule` table holds casbin's standard `(ptype, v0..v5)` row
//! format. This repository provides the CRUD operations the casbin adapter
//! needs, without coupling the storage layer to casbin itself.

use std::fmt;
use std::str::FromStr;

use turso::{Connection, Value, params_from_iter};

use crate::error::{Result, StorageError};
use crate::macros::sql;
use crate::sql::{ToSql, get};

/// Whether a rule is a policy (`p`) or a grouping (`g`) rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyType {
    /// A policy rule (`p`).
    Policy,
    /// A grouping (role) rule (`g`).
    Grouping,
}

impl PolicyType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Policy => "p",
            Self::Grouping => "g",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self> {
        match value {
            "p" => Ok(Self::Policy),
            "g" => Ok(Self::Grouping),
            other => Err(StorageError::UnknownPolicyType(other.to_owned())),
        }
    }
}

impl fmt::Display for PolicyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db())
    }
}

impl FromStr for PolicyType {
    type Err = StorageError;
    fn from_str(s: &str) -> Result<Self> {
        Self::from_db(s)
    }
}

impl ToSql for PolicyType {
    fn to_sql(&self) -> Value {
        Value::Text(self.as_db().to_owned())
    }
}

/// One policy or grouping rule, in casbin's flat string format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    /// Whether this is a policy or grouping rule.
    pub ptype: PolicyType,
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
            let ptype = PolicyType::from_db(&ptype)?;
            let mut values = Vec::with_capacity(6);
            for i in 1..=6 {
                values.push(get::<String>(&row, i)?);
            }
            rules.push(PolicyRule { ptype, values });
        }
        Ok(rules)
    }

    /// Inserts a single rule.
    #[tracing::instrument(level = "info", skip(self, values))]
    pub async fn insert(&self, ptype: PolicyType, values: &[String]) -> Result<()> {
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
        Ok(())
    }

    /// Deletes all rules matching `ptype` and `values` exactly. Returns how
    /// many were removed.
    #[tracing::instrument(level = "info", skip(self, values))]
    pub async fn delete(&self, ptype: PolicyType, values: &[String]) -> Result<usize> {
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
        ptype: PolicyType,
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

    /// Clears the table then inserts all `rules` in a single transaction.
    /// On failure the table is left unchanged.
    #[tracing::instrument(level = "info", skip(self, rules))]
    pub async fn replace_all(&self, rules: &[(PolicyType, Vec<String>)]) -> Result<()> {
        let tx = self
            .connection
            .unchecked_transaction()
            .await
            .map_err(StorageError::from_turso)?;
        self.clear().await?;
        for (ptype, values) in rules {
            self.insert(*ptype, values).await?;
        }
        tx.commit().await.map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Inserts multiple rules in a single transaction.
    #[tracing::instrument(level = "info", skip(self, rules))]
    pub async fn insert_batch(&self, rules: &[(PolicyType, Vec<String>)]) -> Result<()> {
        let tx = self
            .connection
            .unchecked_transaction()
            .await
            .map_err(StorageError::from_turso)?;
        for (ptype, values) in rules {
            self.insert(*ptype, values).await?;
        }
        tx.commit().await.map_err(StorageError::from_turso)?;
        Ok(())
    }

    /// Deletes multiple exact-match rules in a single transaction. Returns the
    /// total number of rows removed.
    #[tracing::instrument(level = "info", skip(self, rules))]
    pub async fn delete_batch(&self, rules: &[(PolicyType, Vec<String>)]) -> Result<usize> {
        let tx = self
            .connection
            .unchecked_transaction()
            .await
            .map_err(StorageError::from_turso)?;
        let mut total = 0;
        for (ptype, values) in rules {
            total += self.delete(*ptype, values).await?;
        }
        tx.commit().await.map_err(StorageError::from_turso)?;
        Ok(total)
    }
}
