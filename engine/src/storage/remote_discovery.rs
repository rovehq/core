use anyhow::{Context, Result};
use sdk::{NodeIdentity, NodeProfile, RemoteDiscoveryCandidate, RemoteTransportRecord};
use sqlx::{Row, SqlitePool};

pub struct RemoteDiscoveryRepository {
    pool: SqlitePool,
}

impl RemoteDiscoveryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn upsert_candidate(&self, candidate: &RemoteDiscoveryCandidate) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO remote_discovery_candidates
               (candidate_id, transport_kind, network_id, member_id, member_name, node_name_hint,
                node_identity_json, node_profile_json, assigned_addresses_json, transports_json, last_seen_at,
                controller_access, paired_node_name, trusted)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(candidate_id) DO UPDATE SET
                 transport_kind = excluded.transport_kind,
                 network_id = excluded.network_id,
                 member_id = excluded.member_id,
                 member_name = excluded.member_name,
                 node_name_hint = excluded.node_name_hint,
                 node_identity_json = excluded.node_identity_json,
                 node_profile_json = excluded.node_profile_json,
                 assigned_addresses_json = excluded.assigned_addresses_json,
                 transports_json = excluded.transports_json,
                 last_seen_at = excluded.last_seen_at,
                 controller_access = excluded.controller_access,
                 paired_node_name = excluded.paired_node_name,
                 trusted = excluded.trusted"#,
        )
        .bind(&candidate.candidate_id)
        .bind(&candidate.transport_kind)
        .bind(&candidate.network_id)
        .bind(&candidate.member_id)
        .bind(&candidate.member_name)
        .bind(&candidate.node_name_hint)
        .bind(serialize_json(&candidate.identity)?)
        .bind(serialize_json(&candidate.profile)?)
        .bind(serialize_json(&candidate.assigned_addresses)?)
        .bind(serialize_json(&candidate.transports)?)
        .bind(candidate.last_seen_at)
        .bind(if candidate.controller_access { 1_i64 } else { 0_i64 })
        .bind(&candidate.paired_node_name)
        .bind(if candidate.trusted { 1_i64 } else { 0_i64 })
        .execute(&self.pool)
        .await
        .context("Failed to upsert remote discovery candidate")?;
        Ok(())
    }

    pub async fn list_candidates(
        &self,
        transport_kind: &str,
    ) -> Result<Vec<RemoteDiscoveryCandidate>> {
        let rows = sqlx::query(
            r#"SELECT candidate_id, transport_kind, network_id, member_id, member_name, node_name_hint,
                      node_identity_json, node_profile_json, assigned_addresses_json, transports_json, last_seen_at,
                      controller_access, paired_node_name, trusted
               FROM remote_discovery_candidates
               WHERE transport_kind = ?
               ORDER BY COALESCE(node_name_hint, member_name, member_id) ASC"#,
        )
        .bind(transport_kind)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list remote discovery candidates")?;

        rows.into_iter().map(map_candidate).collect()
    }

    pub async fn get_candidate(
        &self,
        candidate_id: &str,
    ) -> Result<Option<RemoteDiscoveryCandidate>> {
        let row = sqlx::query(
            r#"SELECT candidate_id, transport_kind, network_id, member_id, member_name, node_name_hint,
                      node_identity_json, node_profile_json, assigned_addresses_json, transports_json, last_seen_at,
                      controller_access, paired_node_name, trusted
               FROM remote_discovery_candidates
               WHERE candidate_id = ?"#,
        )
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch remote discovery candidate")?;

        row.map(map_candidate).transpose()
    }

    pub async fn prune_transport_candidates(
        &self,
        transport_kind: &str,
        network_id: Option<&str>,
        keep_ids: &[String],
    ) -> Result<()> {
        let rows = sqlx::query(
            r#"SELECT candidate_id
               FROM remote_discovery_candidates
               WHERE transport_kind = ?
                 AND (? IS NULL OR network_id = ?)"#,
        )
        .bind(transport_kind)
        .bind(network_id)
        .bind(network_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to inspect discovery candidates for pruning")?;

        for row in rows {
            let candidate_id: String = row.get("candidate_id");
            if keep_ids.iter().any(|id| id == &candidate_id) {
                continue;
            }
            sqlx::query("DELETE FROM remote_discovery_candidates WHERE candidate_id = ?")
                .bind(candidate_id)
                .execute(&self.pool)
                .await
                .context("Failed to prune remote discovery candidate")?;
        }

        Ok(())
    }
}

fn serialize_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).context("Failed to serialize discovery JSON field")
}

fn map_candidate(row: sqlx::sqlite::SqliteRow) -> Result<RemoteDiscoveryCandidate> {
    let identity =
        deserialize_json::<Option<NodeIdentity>>(&row.get::<String, _>("node_identity_json"))?;
    let profile =
        deserialize_json::<Option<NodeProfile>>(&row.get::<String, _>("node_profile_json"))?;
    let assigned_addresses =
        deserialize_json::<Vec<String>>(&row.get::<String, _>("assigned_addresses_json"))?;
    let transports =
        deserialize_json::<Vec<RemoteTransportRecord>>(&row.get::<String, _>("transports_json"))?;

    Ok(RemoteDiscoveryCandidate {
        candidate_id: row.get("candidate_id"),
        transport_kind: row.get("transport_kind"),
        network_id: row.get("network_id"),
        member_id: row.get("member_id"),
        member_name: row.get("member_name"),
        node_name_hint: row.get("node_name_hint"),
        identity,
        profile,
        assigned_addresses,
        last_seen_at: row.get("last_seen_at"),
        controller_access: row.get::<i64, _>("controller_access") != 0,
        paired_node_name: row.get("paired_node_name"),
        trusted: row.get::<i64, _>("trusted") != 0,
        transports,
    })
}

fn deserialize_json<T>(value: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(value)
        .with_context(|| format!("Failed to parse discovery JSON '{}'", value))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::storage::Database;

    use super::*;

    #[tokio::test]
    async fn upsert_and_list_candidates() {
        let temp = TempDir::new().expect("temp");
        let database = Database::new(&temp.path().join("remote-discovery.db"))
            .await
            .expect("database");
        let repo = database.remote_discovery();

        repo.upsert_candidate(&RemoteDiscoveryCandidate {
            candidate_id: "zerotier:net:member".to_string(),
            transport_kind: "zerotier".to_string(),
            network_id: Some("net".to_string()),
            member_id: "member".to_string(),
            member_name: Some("member".to_string()),
            node_name_hint: Some("home-mac".to_string()),
            identity: None,
            profile: None,
            assigned_addresses: vec!["10.10.10.2".to_string()],
            last_seen_at: 1_700_000_000,
            controller_access: true,
            paired_node_name: None,
            trusted: false,
            transports: vec![RemoteTransportRecord {
                kind: "zerotier".to_string(),
                address: "10.10.10.2".to_string(),
                base_url: Some("http://10.10.10.2:47630".to_string()),
                network_id: Some("net".to_string()),
                reachable: true,
                latency_ms: Some(12),
                last_checked_at: Some(1_700_000_000),
                last_error: None,
            }],
        })
        .await
        .expect("upsert");

        let candidates = repo.list_candidates("zerotier").await.expect("list");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].node_name_hint.as_deref(), Some("home-mac"));
    }
}
