# ASH Analysis — Active Session History

ASH samples every active (non-idle) session once per second into an in-memory ring (`v$active_session_history`, ~last hour) and flushes a 1-in-10 subsample to `dba_hist_active_sess_history` (AWR retention). It fills the gap between AWR (hourly, coarse) and v$ views (now only): retrospective, second-level analysis.

Derived metric **AAS** = sampled active sessions / seconds. AAS above the CPU core count means saturation; split by wait class says why.

## Real-Time ASH (last hour) — Tools

- `oracle_ash_aas_trend` (minutes param) → per-minute AAS with on-CPU / User I/O / Concurrency / Commit / Application splits. Spikes locate *when*; the class split hints *why*.
- `oracle_ash_top_sql` → which sql_ids consumed the samples (with plan hash — a change here is a plan flip).
- `oracle_ash_top_events` → the wait-event structure of the window.
- `oracle_ash_blocking` (minutes param) → blocker/waiter sid pairs with sample counts; catches contention that has already resolved (invisible to `oracle_blocking_sessions`). Resolve sids with `oracle_list_active_sessions`.
- `oracle_ash_session_timeline` (sid, minutes) → second-by-second event/sql/blocker trail for one session — the forensic tool for "it was slow at 14:30".

Key columns behind these tools: `SAMPLE_TIME`, `SESSION_ID`, `SQL_ID`, `SQL_PLAN_HASH_VALUE`, `SESSION_STATE` (ON CPU/WAITING), `EVENT`, `WAIT_CLASS`, `CURRENT_OBJ#` (hot object), `BLOCKING_SESSION`, `MODULE`/`ACTION`.

## Historical ASH (dba_hist_active_sess_history)

For incidents older than ~1 hour. The disk store keeps every 10th sample — multiply counts by 10 to approximate seconds.

*No dedicated tool — run via `oracle_execute_sql`:* incident-window analysis (substitute the window):

```sql
-- Top events during the incident window (x10 = approx seconds)
SELECT event, wait_class,
       COUNT(*)                    AS samples,
       ROUND(COUNT(*) * 10 / 3600, 2) AS approx_aas
FROM   dba_hist_active_sess_history
WHERE  sample_time BETWEEN
         TO_TIMESTAMP('2026-03-05 02:00:00', 'YYYY-MM-DD HH24:MI:SS')
     AND TO_TIMESTAMP('2026-03-05 03:00:00', 'YYYY-MM-DD HH24:MI:SS')
  AND  session_type = 'FOREGROUND'
GROUP  BY event, wait_class
ORDER  BY samples DESC;

-- Top SQL + hot objects in the same window
SELECT ash.sql_id, o.owner, o.object_name, o.object_type, COUNT(*) AS wait_samples
FROM   dba_hist_active_sess_history ash
LEFT   JOIN dba_objects o ON o.object_id = ash.current_obj#
WHERE  ash.sample_time BETWEEN
         TO_TIMESTAMP('2026-03-05 02:00:00', 'YYYY-MM-DD HH24:MI:SS')
     AND TO_TIMESTAMP('2026-03-05 03:00:00', 'YYYY-MM-DD HH24:MI:SS')
  AND  ash.session_type = 'FOREGROUND'
GROUP  BY ash.sql_id, o.owner, o.object_name, o.object_type
ORDER  BY wait_samples DESC;
```

*No dedicated tool — run via `oracle_execute_sql`:* blocking chain reconstruction (root blocker = the session not waiting on another session):

```sql
SELECT LPAD(' ', 2*(LEVEL-1)) || session_id AS session_tree,
       blocking_session, event, sql_id, sample_time
FROM   v$active_session_history
WHERE  sample_time > SYSDATE - 10/1440
START  WITH blocking_session IS NULL AND session_state = 'WAITING'
CONNECT BY PRIOR session_id = blocking_session
       AND PRIOR sample_time = sample_time;
```

## ASH Reports (operational, read-only)

```sql
-- Interactive: @?/rdbms/admin/ashrpt.sql
-- Programmatic (substitute window):
SELECT output
FROM   TABLE(DBMS_WORKLOAD_REPOSITORY.ASH_REPORT_HTML(
         l_dbid => (SELECT dbid FROM v$database), l_inst_num => 1,
         l_btime => TO_DATE('2026-03-05 02:00','YYYY-MM-DD HH24:MI'),
         l_etime => TO_DATE('2026-03-05 03:00','YYYY-MM-DD HH24:MI')));
```

Report sections to read in order: Top User Events → Top SQL with Top Events → Top Sessions → Top Objects/Files → Activity Over Time (when it started/ended).

## Incident Workflow: "slow between 9:00 and 9:15"

1. Confirm the spike: `oracle_ash_aas_trend` (or the historical window query above).
2. Identify the SQL: `oracle_ash_top_sql` (or window query) → `oracle_sql_text` for the statement.
3. Identify who: filter the window query by `sql_id`, group by `session_id`/`module`/`machine`.
4. What they waited on: group the window by `event`; `oracle_ash_session_timeline` for one session.
5. Pull the plan of that era: `DBMS_XPLAN.DISPLAY_AWR` (see [top-sql-queries.md](../monitoring/top-sql-queries.md)).
6. Check whether the plan changed since: `oracle_sql_stats_history`.

## ASH vs AWR

| Need | Use |
|---|---|
| Incident < 60 min ago, second-level detail | `v$active_session_history` → the ASH tools |
| Incident days/weeks ago | `dba_hist_active_sess_history` (1-in-10 resolution) |
| Workload trends, before/after comparison | AWR ([awr-reports.md](awr-reports.md)) |

## Best Practices

- **Filter `session_type = 'FOREGROUND'`** unless you specifically want background housekeeping waits.
- **Annotate apps with `DBMS_APPLICATION_INFO.SET_MODULE/SET_ACTION`** — ASH becomes dramatically more useful.
- **Multiply by 10** for disk-based ASH; don't query `DBA_HIST` for the last few minutes (not yet flushed).
- **Trace blocking chains to the root** — `blocking_session` may itself be blocked.
- **Join `CURRENT_OBJ#` to `dba_objects`** for User I/O waits to name the hot segment.

## Common Mistakes

- Reading per-row timing literally — samples jitter; aggregate over ranges.
- Comparing in-memory ASH directly with disk ASH without the 10x factor.
- Stopping at the direct blocker instead of the root of the chain.

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c. `DBMS_XPLAN.DISPLAY_AWR` is deprecated from 23ai in favor of `DISPLAY_WORKLOAD_REPOSITORY` — keep the old form for 19c estates.

## See Also

- [Wait Events](wait-events.md) — cumulative vs sampled view
- [AWR Reports](awr-reports.md) — snapshot-level analysis

## Sources

- [19c Reference — V$ACTIVE_SESSION_HISTORY](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-ACTIVE_SESSION_HISTORY.html)
- [19c Reference — DBA_HIST_ACTIVE_SESS_HISTORY](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/DBA_HIST_ACTIVE_SESS_HISTORY.html)
- [19c Performance Tuning Guide](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgdba/)
