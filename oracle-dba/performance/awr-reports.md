# AWR Reports — Automatic Workload Repository

AWR snapshots cumulative v$ statistics hourly (default) into SYSAUX (10g+). AWR reports diff two snapshots into a workload summary — the standard artifact for performance incidents and before/after comparisons.

Core metrics: **DB Time** = CPU + non-idle wait time of all foreground sessions; **AAS** = DB Time / elapsed seconds — AAS at or above the CPU count means saturation.

## Tool-Assisted AWR Analysis

- `oracle_awr_top_sql` (days param) → top 15 SQL by total elapsed from `dba_hist_sqlstat` — the "SQL ordered by Elapsed" section on demand, without generating a report.
- `oracle_sql_stats_history` (sql_id, days) → per-snapshot performance and plan hash of one statement — the regression detector.
- `oracle_tablespace_growth_history` → weekly per-tablespace growth from AWR.
- `oracle_ash_aas_trend` → minute-level AAS for the recent window (AWR granularity is hourly; ASH fills between snapshots).

*No dedicated tool — run via `oracle_execute_sql`:* snapshot inventory and hard-parse trend:

```sql
SELECT snap_id,
       TO_CHAR(begin_interval_time, 'YYYY-MM-DD HH24:MI') AS begin_time
FROM   dba_hist_snapshot
WHERE  begin_interval_time >= SYSDATE - 1
ORDER  BY snap_id;

-- Hard parses per snapshot (validate Load Profile findings)
SELECT snap_id, hard_parses,
       hard_parses / NULLIF(elapsed_sec, 0) AS hard_parses_per_sec
FROM (SELECT sn.snap_id,
             s.value - LAG(s.value) OVER (ORDER BY sn.snap_id) AS hard_parses,
             (sn.end_interval_time - sn.begin_interval_time) * 86400 AS elapsed_sec
      FROM   dba_hist_sysstat s
      JOIN   dba_hist_snapshot sn USING (snap_id, dbid, instance_number)
      WHERE  s.stat_name = 'hard parses')
WHERE hard_parses IS NOT NULL;
```

## Generating Reports (operational, read-only)

```bash
@?/rdbms/admin/awrrpt.sql      # single instance, interactive
@?/rdbms/admin/awrgrpt.sql     # RAC global
@?/rdbms/admin/awrddrpt.sql    # compare-period (baseline vs current)
```

```sql
-- Programmatic text report between two snapshots
SELECT output
FROM   TABLE(DBMS_WORKLOAD_REPOSITORY.AWR_REPORT_TEXT(
         l_dbid => (SELECT dbid FROM v$database), l_inst_num => 1,
         l_bid => 200, l_eid => 201));
-- ...AWR_REPORT_HTML for the browser-friendly version
```

## Snapshot Management (operational — modifies AWR, approval required)

```sql
EXEC DBMS_WORKLOAD_REPOSITORY.CREATE_SNAPSHOT();   -- before/after a change

BEGIN                                              -- e.g. 30-min interval, 14-day retention
  DBMS_WORKLOAD_REPOSITORY.MODIFY_SNAPSHOT_SETTINGS(
    retention => 14 * 24 * 60,   -- minutes
    interval  => 30);            -- minutes
END;
/

BEGIN                                              -- reclaim SYSAUX space
  DBMS_WORKLOAD_REPOSITORY.DROP_SNAPSHOT_RANGE(low_snap_id => 100, high_snap_id => 150);
END;
/

SELECT snap_interval, retention FROM dba_hist_wr_control;   -- current settings
```

## Reading the Report — What to Check, In Order

1. **Header** — right database, host, CPUs, window? Verify before concluding.
2. **Load Profile** — per-second DB Time (= AAS), Redo size, Logical reads, **Hard parses (>100/s = literal SQL or pool churn — see `oracle_literal_sql`)**, Logons (connection churn).
3. **Instance Efficiency** — Buffer/Library/In-memory Sort/Soft Parse % targets >95% (concern <90%); Buffer/Redo Nowait % ≈ 100%.
4. **Top Foreground Events** — map to families in [wait-events.md](wait-events.md): sequential/scattered read, log file sync, buffer busy waits, enq: TX, library cache mutex.
5. **SQL Statistics** — Elapsed first, then CPU/Gets/Reads/Executions/Parse-calls. Programmatic version: `oracle_awr_top_sql` + `oracle_sql_text`.
6. **Segments Statistics** — hot tables/indexes by physical reads and buffer gets.
7. **Library/Dictionary Cache** — miss ratios >1–2% point at shared pool pressure (`oracle_shared_pool_top`).

Bottleneck signatures: **CPU-bound** (DB CPU ≈ DB Time, high parse CPU) → bad SQL/stats; **I/O-bound** (sequential/scattered read top, high physical reads) → SQL access paths/storage; **contention** (enq:/buffer busy/latch) → app design, hot blocks; **memory pressure** (library cache misses, free buffer waits) → sizing/literal SQL; **commit overhead** (log file sync + high redo/s) → batching or redo storage.

## Baselines (operational)

```sql
BEGIN
  DBMS_WORKLOAD_REPOSITORY.CREATE_BASELINE(
    start_snap_id => 200, end_snap_id => 210,
    baseline_name => 'PRE_PATCH_BASELINE', expiration => 30);   -- NULL = keep forever
END;
/
SELECT baseline_name, start_snap_id, end_snap_id FROM dba_hist_baseline;
```

Always baseline before patches/upgrades, then run the compare-period report (`awrddrpt.sql`) to prove impact.

## Best Practices

- **Snapshot before and after every change** — enables a precise before/after diff.
- **HTML for humans, TEXT for automation.**
- **AWR is for trends and post-incident review** — for a live fire use the v$/ASH tools first ([top-sql-queries](../monitoring/top-sql-queries.md)).
- **Watch SYSAUX growth** from AWR retention: *No dedicated tool — run via `oracle_execute_sql`:* `SELECT occupant_name, ROUND(space_usage_kbytes/1024) AS mb FROM v$sysaux_occupants ORDER BY space_usage_kbytes DESC;`

## Common Mistakes

- Comparing snapshots of very different durations or workload windows (month-end vs Tuesday).
- Reading "Top SQL by elapsed" without per-execution normalization — check avg per exec.
- Trusting one AWR report as "the performance of the system" — correlate with ASH for the actual incident minutes.
- Dropping snapshot ranges that back a baseline you still need.

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c unless noted; AWR defaults (interval 60 min, retention 8 days) differ on some platforms/editions — check `dba_hist_wr_control`.

## See Also

- [ASH Analysis](ash-analysis.md) — second-level view between snapshots
- [Top SQL](../monitoring/top-sql-queries.md) — live cursor-cache ranking

## Sources

- [19c Performance Tuning Guide — AWR](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgdba/)
- [19c PL/SQL Packages — DBMS_WORKLOAD_REPOSITORY](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_WORKLOAD_REPOSITORY.html)
