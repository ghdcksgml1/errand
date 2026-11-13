//! Read-only fallback for rollouts Codex moved to its paginated history. This private on-disk schema is
//! optional: an older or unrecognized database yields no turn, and never changes Codex's records.

use errand_core::Timestamp;
use errand_core::sessions::{Turn, fold::fold_codex_item};
use rusqlite::Connection;
use serde_json::Value;

pub(super) fn latest_turn(db: &Connection, thread: &str) -> Option<(Turn, Option<Timestamp>)> {
    // Keep turn status and its items from the same snapshot while Codex appends to its WAL.
    let snapshot = db.unchecked_transaction().ok()?;
    let (id, status, started, completed, ordinal): (String, String, Option<i64>, Option<i64>, i64) = snapshot.query_row(
        "SELECT turn_id, status, started_at, completed_at, rollout_ordinal FROM thread_turns WHERE thread_id = ? ORDER BY rollout_ordinal DESC LIMIT 1",
        [thread], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).ok()?;
    let mut turn = Turn { started_at: started.and_then(|time| Timestamp::from_second(time).ok()), ..Turn::default() };
    // Automated continuation can open a turn without a new user message. As in the rollout reader, keep the
    // last actual request instead of falling back to the very first message of the conversation.
    let previous: Option<(String, i64)> = snapshot.query_row(
        "SELECT item_json, created_at_ms FROM thread_items WHERE thread_id = ?1 AND item_type = 'userMessage' \
         AND rollout_ordinal < ?2 ORDER BY rollout_ordinal DESC LIMIT 1",
        (thread, ordinal), |row| Ok((row.get(0)?, row.get(1)?)),
    ).ok();
    if let Some((json, ms)) = previous
        && let (Ok(Value::Object(item)), Ok(at)) = (serde_json::from_str::<Value>(&json), Timestamp::from_millisecond(ms))
    {
        fold_codex_item(&mut turn, &item, at);
    }
    let mut statement = snapshot.prepare(
        "SELECT item_json, created_at_ms FROM thread_items WHERE thread_id = ?1 AND turn_id = ?2 \
         AND item_type IN ('userMessage', 'agentMessage') ORDER BY rollout_ordinal",
    ).ok()?;
    let mut items = statement.query((thread, &id)).ok()?;
    while let Some(row) = items.next().ok()? {
        let (Ok(json), Ok(ms)) = (row.get::<_, String>(0), row.get::<_, i64>(1)) else { continue };
        if let (Ok(Value::Object(item)), Ok(at)) = (serde_json::from_str::<Value>(&json), Timestamp::from_millisecond(ms)) {
            fold_codex_item(&mut turn, &item, at);
        }
    }
    turn.running = status == "inProgress";
    turn.aborted = status == "interrupted";
    turn.finished_at = completed.filter(|_| matches!(status.as_str(), "completed" | "interrupted"))
        .and_then(|time| Timestamp::from_second(time).ok());
    if !turn.running {
        turn.question = None;
        turn.question_is_async = false;
    }
    if status != "completed" {
        turn.result = None;
    }
    // A failure or unknown status never becomes a successful answer just because it has a completion time.
    let activity_ms: Option<i64> = snapshot.query_row(
        "SELECT MAX(COALESCE(completed_at_ms, started_at_ms, created_at_ms)) FROM thread_items WHERE thread_id = ?1 AND turn_id = ?2",
        (thread, &id), |row| row.get(0),
    ).ok().flatten();
    let modified = activity_ms.and_then(|time| Timestamp::from_millisecond(time).ok()).or(turn.finished_at).or(turn.started_at);
    Some((turn, modified))
}
