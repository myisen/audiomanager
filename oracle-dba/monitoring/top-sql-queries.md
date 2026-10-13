# Top SQL Queries and SQL Monitoring

Finding and fixing the most resource-hungry SQL is the highest-leverage DBA activity. The agent flow: rank cached SQL (`oracle_top_sql`) → drill into text/plan/history → use AWR for anything older than the cursor cache → SQL Monitor for per-operation runtime detail on a live execution.

## Ranking Current SQL (v$sql / v$sqlarea)

Call `oracle_top_sql` — one tool, five rankings via the `sort_by` parameter:

| sort_by | Read it as | Typical cause when it tops the list |
|---------|-----------|--------------------------------------|
| `ELAPSED` (default) | wall-clock impact incl. waits | wait-bound SQL, or one huge query |
| `CPU` | CPU burn | full scans, big sorts, large result sets |
| `BUFFER_GETS` | logical I/O | inefficient access path, memory pressure |
| `DISK_READS` | physical I/O | missing index, cold cache, full scans |
| `EXECUTIONS` | frequency | cheap-but-hot statements; batching/result-cache candidates |

Read `avg_ms_per_exec` together with `executions`: high total + low per-exec = frequency problem (fix in the app); low total + high per-exec = per-execution problem (fix the plan). Interpreting deltas between dimensions (elapsed vs CPU = waiting; disk_reads/buffer_gets = cache-miss ratio) tells you which way to dig. `v$sqlarea` aggregates child cursors; use `v$sql` granularity when children diverge.

## Drill Into One Statement

1. `oracle_sql_text` (sql_id) — full text, untruncated (`SQL_TEXT` previews are cut at 120/1000 chars — never tune from a preview).
2. `oracle_sql_plan` (sql_id) — cached plan per child cursor; several children with different shapes = plan flip.
3. `oracle_sql_stats_history` (sql_id, days) — per-snapshot avg ms, gets, and **plan_hash_value** from AWR: a jump in avg time coinciding with a new plan hash is your regression point.

## AWR Historical Top SQL

`v$sql` only holds cursors not yet aged out. For "what dominated last week" call `oracle_awr_top_sql` (days param; default 1) — top 15 by total elapsed from `dba_hist_sqlstat`.

*No dedicated tool — run via `oracle_execute_sql`:* available AWR snapshots:

```sql
SELECT snap_id, begin_interval_time, end_interval_time
FROM   dba_hist_snapshot
ORDER  BY snap_id DESC
FETCH FIRST 24 ROWS ONLY;
```

*No dedicated tool — run via `oracle_execute_sql`:* plan as recorded in AWR (survives cursor aging):

```sql
SELECT plan_hash_value, timestamp, operation, options,
       object_owner, object_name, cost, cardinality
FROM   dba_hist_sql_plan
WHERE  sql_id = 'abc123def456g'
ORDER  BY plan_hash_value, id;
```

Formatted AWR plan (operational, read-only):

```sql
SELECT * FROM TABLE(DBMS_XPLAN.display_awr(sql_id => 'abc123def456g', format => 'ALL'));
```

## Real-Time SQL Monitoring (v$sql_monitor)

Auto-activates for statements >5s of CPU/IO, parallel execution, or `/*+ MONITOR */`. *No dedicated tool — run via `oracle_execute_sql`:*

```sql
-- Currently executing monitored SQL
SELECT sql_id, sql_exec_id, status, username,
       ROUND(elapsed_time/1e6, 1) AS elapsed_sec,
       ROUND(cpu_time/1e6, 1)     AS cpu_sec,
       buffer_gets, disk_reads,
       ROUND(physical_read_bytes/1048576, 1) AS phys_read_mb,
       px_servers_allocated,
       SUBSTR(sql_text, 1, 80) AS sql_preview
FROM   v$sql_monitor
WHERE  status = 'EXECUTING'
ORDER  BY elapsed_time DESC;
```

Per-operation detail for a live execution (`v$sql_plan_monitor`, join on `sql_id` + `sql_exec_id` from above):

```sql
SELECT plan_line_id, plan_operation, plan_options, plan_object_name,
       output_rows, starts,
       ROUND(elapsed_time/1e6, 3) AS elapsed_sec,
       workarea_mem, workarea_tempseg,
       physical_read_requests, physical_write_requests
FROM   v$sql_plan_monitor
WHERE  sql_id = 'abc123def456g' AND sql_exec_id = 16777216
ORDER  BY plan_line_id;
```

Formatted report (TEXT for terminal/email, ACTIVE for interactive HTML):

```sql
SELECT DBMS_SQLTUNE.report_sql_monitor(sql_id => 'abc123def456g', type => 'TEXT') AS report FROM dual;
```

## Standard Workflow

1. `oracle_top_sql` (sort_by=ELAPSED) — the offender list. Check `LAST_ACTIVE_TIME` via execute_sql before tuning something that may not have run for days.
2. Recurring or already-aged-out? `oracle_awr_top_sql` with a wider window.
3. `oracle_sql_text` → `oracle_sql_plan` → `oracle_sql_stats_history` — what it is, how it executes, when it changed.
4. Live and long-running? SQL Monitor report (above) or `oracle_long_running_sql` for progress/ETA; `oracle_ash_session_timeline` for what the session waited on.
5. Recent activity context: `oracle_ash_top_sql` / `oracle_ash_aas_trend`.

## Best Practices

1. **Rank by elapsed time first** — it captures waits; a CPU-light query that blocks others for 30 minutes outranks a CPU burner.
2. **Always normalize by executions** — decide whether to cut frequency (app-side) or per-exec cost (plan-side).
3. **Treat `plan_hash_value` change + latency jump as the regression signature.**
4. **Prefer high disk_reads/exec over high buffer_gets** — physical I/O is orders of magnitude costlier.
5. **Watch `LOADS`/`INVALIDATIONS` vs executions** — repeated hard parsing hurts scalability even when elapsed time looks fine (see `oracle_literal_sql`).
6. **Save SQL Monitor HTML reports for major incidents** — completed-statement monitoring ages out (~30 days).
7. **AWR for trends, v$ for live fires** — snapshots are hourly; during a crisis use `oracle_top_sql`, `oracle_list_active_sessions`, `oracle_session_wait_details`.

## Common Mistakes

- **Tuning a cursor that no longer runs** — check `LAST_ACTIVE_TIME` first.
- **Analyzing truncated SQL previews** — always pull full text via `oracle_sql_text`.
- **Comparing AWR deltas across snapshots of different durations** without normalizing.
- **Trusting hourly AWR during a live incident** — use real-time views instead.

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c unless a higher minimum is noted; `V$SQL.SQL_FULLTEXT` (CLOB) is 11g+.

## See Also

- [ASH Analysis](ash-analysis.md) — what sessions actually waited on
- [Explain Plan](explain-plan.md) — reading plans step by step

## Sources

- [19c SQL Tuning Guide — Identifying High-Load SQL](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgsql/identifying-high-load-sql.html)
- [19c Reference — V$SQLAREA](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-SQLAREA.html)
- [19c Reference — DBA_HIST_SQLSTAT](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/DBA_HIST_SQLSTAT.html)
