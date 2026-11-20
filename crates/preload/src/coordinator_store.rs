use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use rivetkit::{BindParam, ColumnValue, Ctx};

use super::{
    policy::validation_policy, validate_coordinator_state_with_policy, PreloadCoordinatorActor,
    PreloadCoordinatorState, PreloadValidationPolicy,
};

const SCHEMA_VERSION: i64 = 1;
const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(5);

// This actor has its own physical Rivet SQLite database. These tables and its
// migration ladder belong to the actor layer, independently of VM/VFS schemas.
pub(super) async fn load_or_initialize(
    ctx: &Ctx<PreloadCoordinatorActor>,
    initialize: impl FnOnce() -> Result<PreloadCoordinatorState>,
) -> Result<PreloadCoordinatorState> {
    let policy = validation_policy();
    let transaction = ctx
        .sql()
        .begin_named_transaction(Some("agentos-preload"), Some(TRANSACTION_TIMEOUT))
        .await
        .context("begin preload coordinator migration")?;
    let result = async {
        transaction
            .exec(
                "CREATE TABLE IF NOT EXISTS agentos_actor_schema_version (
            id INTEGER PRIMARY KEY CHECK (id = 1), version INTEGER NOT NULL
        ) STRICT",
            )
            .await?;
        transaction
            .exec("INSERT OR IGNORE INTO agentos_actor_schema_version (id, version) VALUES (1, 0)")
            .await?;
        let rows = transaction
            .exec("SELECT version FROM agentos_actor_schema_version WHERE id = 1")
            .await?
            .rows;
        let version = match rows.as_slice() {
            [row] => match row.as_slice() {
                [ColumnValue::Integer(version)] => *version,
                _ => bail!("preload coordinator schema version has an invalid type"),
            },
            _ => bail!("preload coordinator schema version row is missing or ambiguous"),
        };
        if let Some(encoded) = encode_new_state_for_schema(version, initialize, policy)? {
            transaction
                .exec(
                    "CREATE TABLE agentos_actor_preload_state (
                id INTEGER PRIMARY KEY CHECK (id = 1), state_json TEXT NOT NULL
            ) STRICT",
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO agentos_actor_preload_state (id, state_json) VALUES (1, ?)",
                    Some(vec![BindParam::Text(encoded)]),
                )
                .await?;
            transaction
                .exec("UPDATE agentos_actor_schema_version SET version = 1 WHERE id = 1")
                .await?;
        }
        let rows = transaction
            .exec("SELECT state_json FROM agentos_actor_preload_state WHERE id = 1")
            .await?
            .rows;
        decode_rows_with_policy(&rows, policy)
    }
    .await;
    match result {
        Ok(state) => {
            transaction
                .commit()
                .await
                .context("commit preload coordinator migration")?;
            Ok(state)
        }
        Err(error) => match transaction.rollback().await {
            Ok(()) => Err(error),
            Err(rollback) => Err(error.context(format!(
                "rollback preload coordinator migration failed: {rollback:#}"
            ))),
        },
    }
}

// Schema validation precedes the lazy initializer. Existing rows never consult
// stale creation input; missing/corrupt v1 rows remain errors, never recreated.
pub(super) fn encode_new_state_for_schema(
    version: i64,
    initialize: impl FnOnce() -> Result<PreloadCoordinatorState>,
    policy: &PreloadValidationPolicy,
) -> Result<Option<String>> {
    match version {
        0 => encode_with_policy(&initialize()?, policy).map(Some),
        SCHEMA_VERSION => Ok(None),
        _ => bail!(
            "unsupported preload coordinator schema version {version}; expected {SCHEMA_VERSION}"
        ),
    }
}

// Read the authoritative row for each action. A cancelled commit cannot leave
// a stale in-memory state mirror after the database has accepted a mutation.
pub(super) async fn load(ctx: &Ctx<PreloadCoordinatorActor>) -> Result<PreloadCoordinatorState> {
    let rows = ctx
        .sql()
        .exec("SELECT state_json FROM agentos_actor_preload_state WHERE id = 1")
        .await
        .context("load preload coordinator state")?
        .rows;
    decode_rows_with_policy(&rows, validation_policy())
}

pub(super) async fn persist(
    ctx: &Ctx<PreloadCoordinatorActor>,
    state: &PreloadCoordinatorState,
) -> Result<()> {
    // Materialize and validate before constructing/polling an UPDATE. A rejected
    // policy leaves the authoritative saved row and revision unchanged.
    let encoded = encode_with_policy(state, validation_policy())?;
    let result = ctx
        .sql()
        .execute(
            "UPDATE agentos_actor_preload_state SET state_json = ? WHERE id = 1",
            Some(vec![BindParam::Text(encoded)]),
        )
        .await
        .context("persist preload coordinator state")?;
    if result.changes != 1 {
        bail!("preload coordinator state row is missing; expected exactly one durable owner");
    }
    Ok(())
}

#[cfg(test)]
fn encode(state: &PreloadCoordinatorState) -> Result<String> {
    encode_with_policy(state, validation_policy())
}

pub(super) fn encode_with_policy(
    state: &PreloadCoordinatorState,
    policy: &PreloadValidationPolicy,
) -> Result<String> {
    validate_coordinator_state_with_policy(state, policy)?;
    let encoded = serde_json::to_string(state).context("encode preload coordinator state")?;
    check_size_with_policy(encoded.len(), policy)?;
    Ok(encoded)
}

#[cfg(test)]
fn decode_rows(rows: &[Vec<ColumnValue>]) -> Result<PreloadCoordinatorState> {
    decode_rows_with_policy(rows, validation_policy())
}

pub(super) fn decode_rows_with_policy(
    rows: &[Vec<ColumnValue>],
    policy: &PreloadValidationPolicy,
) -> Result<PreloadCoordinatorState> {
    let encoded = match rows {
        [row] => match row.as_slice() {
            [ColumnValue::Text(encoded)] => encoded,
            _ => return Err(anyhow!("preload coordinator state has an invalid type")),
        },
        _ => bail!("preload coordinator state row is missing or ambiguous"),
    };
    check_size_with_policy(encoded.len(), policy)?;
    let state = serde_json::from_str(encoded).context("decode preload coordinator state")?;
    validate_coordinator_state_with_policy(&state, policy)?;
    Ok(state)
}

#[cfg(test)]
fn check_size(bytes: usize) -> Result<()> {
    check_size_with_policy(bytes, validation_policy())
}

fn check_size_with_policy(bytes: usize, policy: &PreloadValidationPolicy) -> Result<()> {
    if bytes > policy.max_state_bytes {
        return Err(super::policy::PreloadLimitViolation::new(
            "preloadPolicy.maxStateBytes".into(),u64::try_from(bytes).context("encoded state size exceeds u64 representation")?,
            u64::try_from(policy.max_state_bytes).context("state budget exceeds u64 representation")?,"bytes",
            "raise maxStateBytes in --preload-policy-json and restart this worker, or reduce retained plan/candidate/process data; coordinator maxCandidates/maxTrackedProcesses/maxPlanEntries are creation-only and are not updated by restart; the saved row is unchanged".into(),
        )?.into());
    }
    if bytes >= policy.max_state_bytes - policy.max_state_bytes / 4 {
        tracing::warn!(observed=bytes,capacity=policy.max_state_bytes,configuration_path="preloadPolicy.maxStateBytes","preload coordinator durable state approaches its byte limit; configure --preload-policy-json before startup");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_state_round_trip_preserves_revision_and_counters() {
        let mut state = crate::initial_coordinator_state(Default::default()).unwrap();
        state.revision = 7;
        state.accepted_usage_batches = 3;
        let restored = decode_rows(&[vec![ColumnValue::Text(encode(&state).unwrap())]]).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn malformed_or_missing_rows_fail_without_defaulting() {
        assert!(decode_rows(&[]).is_err());
        assert!(decode_rows(&[vec![ColumnValue::Integer(1)]]).is_err());
        assert!(decode_rows(&[vec![ColumnValue::Text("{}".into())]]).is_err());
    }

    #[test]
    fn state_limit_reports_the_owning_limit_without_allocating_it() {
        let error = check_size(PreloadValidationPolicy::default().max_state_bytes + 1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("limit_exceeded") && error.contains("maxCandidates"));
    }
}
