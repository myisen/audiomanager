# Wait Events — Diagnosis and Remediation

Every stall records a wait event with name, parameters (P1/P2/P3) and duration. Waits are classified: `User I/O`, `System I/O`, `Commit`, `Concurrency`, `Configuration`, `Network`, `Application`, `Cluster`, `Other` — and `Idle` (always excluded from analysis).

## Tool Flow

1. **What hurts now, cumulatively:** `oracle_wait_events` — top non-idle events since startup with % of total wait time. Sort by *time*, never by count.
2. **What hurts this hour:** `oracle_ash_top_events` + `oracle_ash_aas_trend` — sampled, recent, per-class.
3. **Who is waiting right now:** `oracle_list_active_sessions` (event + SQL text) → `oracle_session_wait_details` for P1/P2/P3 of every non-idle waiter.
4. **Lock-shaped waits:** `oracle_blocking_sessions` (chain + waited object) and `oracle_ash_blocking` for resolved contention.
5. Then remediate per event family below.

*No dedicated tool — run via `oracle_execute_sql`:* wait-duration histogram for one event:

```sql
SELECT wait_time_milli, wait_count,
       ROUND(100 * wait_count / SUM(wait_count) OVER (PARTITION BY event), 2) AS pct
FROM   v$event_histogram
WHERE  event = 'db file sequential read'
ORDER  BY wait_time_milli;
```

## Event Families and Fixes

### db file sequential read (single-block read)

Normal for indexed OLTP; a problem when access path is wrong or the index's clustering factor is terrible. P1=file#, P2=block#, P3=blocks. Map to the hot object live:

*No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT o.owner, o.object_name, o.object_type, COUNT(*) AS waits
FROM   v$session_wait sw
JOIN   dba_extents e ON e.file_id = sw.p1
                     AND sw.p2 BETWEEN e.block_id AND e.block_id + e.blocks - 1
JOIN   dba_objects o ON o.object_id = e.object_id
WHERE  sw.event = 'db file sequential read'
GROUP  BY o.owner, o.object_name, o.object_type
ORDER  BY waits DESC;
```

Fixes: verify plan/`oracle_stale_stats`; rebuild or replace the index (high clustering factor → reorg, see [space-management](../monitoring/space-management.md)); add a covering index; faster storage.

### db file scattered read (multiblock read = full scan)

Fine for analytics, wrong for OLTP. Find the SQL: `oracle_top_sql` (sort_by=DISK_READS) and `oracle_awr_top_sql`. Fixes: index the OLTP query; `ALTER TABLE t CACHE` for small hot tables; partition pruning; parallel query for intentional scans.

### log file sync (commit waits for LGWR)

Call `oracle_instance_load_stats` (redo size, user commits) and check averages:

*No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT ROUND(time_waited_micro / NULLIF(total_waits,0) / 1000, 2) AS avg_sync_ms
FROM   v$system_event WHERE event = 'log file sync';
-- > 5ms is a concern; > 20ms is serious
```

Fixes: faster/less-loaded redo storage; batch commits (row-by-row loops are the usual culprit); bigger redo logs (`oracle_redo_log_config`, `oracle_redo_switches`); hardware-mirror redo instead of extra multiplexing. `COMMIT WRITE NOWAIT` trades durability — only for non-critical staging (operational, approval required):

```sql
ALTER SESSION SET COMMIT_WRITE = 'BATCH,NOWAIT';   -- risk: last commits lost on crash
```

### buffer busy waits / read by other session / latch: cache buffers chains

Concurrent access to the same block: segment header, hot data block, or right-growing index. Map file#/block# with the `dba_extents` query above (event name changed). Fixes: ASSM + multiple freelist groups; reverse-key index for sequence-driven PKs (`CREATE INDEX ... REVERSE` — no range scans after, operational); app redesign/partitioning for genuinely hot rows.

### enq: TX / enq: TM (locks)

| Enqueue | Meaning | Usual cause |
|---|---|---|
| `enq: TX - row lock contention` | row lock | two sessions updating the same row |
| `enq: TM - contention` | table lock | DDL vs DML, **unindexed FK** |
| `enq: HW - contention` | HWM extension | many concurrent segment extensions |

Use `oracle_blocking_sessions` (shows waited object via `row_wait_obj#`) → root blocker via `oracle_ash_blocking`. Unindexed-FK check: `oracle_unindexed_foreign_keys`. Fixes: shorten transaction hold time, commit earlier, `SELECT ... FOR UPDATE NOWAIT` to fail fast; kill of a stale blocker is a recommendation only (human confirms).

### library cache contention (mutex X / lock / pin)

Hard-parse pressure. Check parse counters in `oracle_instance_load_stats` → find literal-SQL families with `oracle_literal_sql` → fix with bind variables in the app. Emergency `ALTER SYSTEM SET CURSOR_SHARING = FORCE` is a workaround with side effects (operational, revert to EXACT after).

### free buffer waits

Buffer cache too small or DBWR too slow. `oracle_memory_advisors` (DB_CACHE rows: read factor vs size) → resize or add `DB_WRITER_PROCESSES` (operational).

### direct path read temp (sorts spilling)

`oracle_temp_by_session` — who spills; `oracle_temp_usage` — how full. Fixes: bigger PGA (`oracle_memory_advisors`, PGA rows), reduce intermediate result sets in the SQL.

## Best Practices

- **Exclude Idle always; rank by time waited, never by wait count.**
- **Correlate event → sql_id → plan**: the event is the symptom, the SQL is the cause.
- **Read P1/P2/P3** — they name the file/block/lock; that is the object-level diagnosis.
- **`db file sequential read` is not automatically bad** — verify the access path first.
- **`log file sync` is not automatically storage** — check commits/sec before blaming disks.

## Common Mistakes

- Killing a blocker without finding why it held the lock — it recurs.
- Ignoring P1/P2/P3 and stopping at event-level diagnosis.
- Treating one huge cumulative number (since startup) as today's problem — use ASH tools for the recent window.

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c unless noted. `CURSOR_SPACE_FOR_TIME` is deprecated (12c+).

## See Also

- [ASH Analysis](ash-analysis.md) — sampled recent waits
- [Top SQL](../monitoring/top-sql-queries.md) — from waits to statements

## Sources

- [19c Reference — V$SESSION_WAIT](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-SESSION_WAIT.html)
- [19c Reference — V$SYSTEM_EVENT](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-SYSTEM_EVENT.html)
- [19c Performance Tuning Guide](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgdba/)
