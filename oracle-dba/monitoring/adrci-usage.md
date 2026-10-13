# ADRCI Usage

`adrci` is Oracle's CLI for the Automatic Diagnostic Repository (ADR, 11g+): filterable access to the alert log, trace files, incidents and problems, IPS packaging for Oracle Support, and retention management. Use it instead of `grep`/`tail` on raw files — predicates run on indexed metadata.

## Finding Your ADR Home

Call `oracle_diag_info` → read `ADR Base`, `ADR Home`, `Diag Alert` (XML log), `Diag Trace` (trace files + text log), `Diag Incident`.

A server hosts one ADR home per product instance (DB instances, listener, ASM). `adrci` can search across several at once:

```bash
adrci> SHOW HOMES
adrci> SET HOMEPATH diag/rdbms/orcl/orcl diag/rdbms/testdb/testdb   # multi-home search
adrci> SET HOMEPATH diag/rdbms/orcl/orcl                            # back to one
```

## Alert Log via adrci

```bash
SHOW ALERT -TAIL 100                 # tail -n
SHOW ALERT -TAIL 20 -F               # tail -f equivalent
SHOW ALERT -P "MESSAGE_TEXT LIKE '%ORA-%'"
SHOW ALERT -P "ORIGINATING_TIMESTAMP > TIMESTAMP '2026-03-06 08:00:00' AND MESSAGE_TEXT LIKE '%ORA-00600%'"
```

For quick triage without OS access, call `oracle_alert_log_errors` (12.1+, param: minutes) instead — it reads the same XML alert content via `v$diag_alert_ext`.

Key predicate columns: `ORIGINATING_TIMESTAMP`, `MESSAGE_TEXT`, `MESSAGE_TYPE` (3=Error, 4=Warning), `COMPONENT_ID`.

## Incidents and Problems

An **incident** is one occurrence of a critical error (its own `incdir_<id>` with trace files); a **problem** groups incidents sharing a **problem key** (e.g. `ORA 600 [kcbz_check_objd_typ_3]`).

```bash
SHOW INCIDENT                        # most recent first
SHOW INCIDENT -MODE DETAIL -P "PROBLEM_KEY LIKE '%ORA-00600%'"
SHOW PROBLEM -MODE DETAIL            # grouped view with counts
```

*No dedicated tool — run via `oracle_execute_sql`:* incident/problem correlation from SQL (11g+):

```sql
SELECT i.incident_id, i.create_time, p.problem_key, i.status
FROM   v$diag_incident i
JOIN   v$diag_problem  p ON i.problem_id = p.problem_id
ORDER BY i.create_time DESC
FETCH FIRST 50 ROWS ONLY;
```

Always note the incident ID when ORA-00600/07445 occurs — it links alert entry to trace directory and IPS package.

## IPS: Packaging for Oracle Support

```bash
IPS CREATE PACKAGE INCIDENT 12345 CORRELATE ALL      # always correlate
IPS CREATE PACKAGE PROBLEM 3
IPS CREATE PACKAGE TIMEWINDOW '2026-03-06 14:00:00' '2026-03-06 15:00:00'
IPS SHOW PACKAGE PACKAGE 1
IPS ADD FILE /path/to/extra.trc PACKAGE 1
IPS GENERATE PACKAGE 1 IN /tmp                       # explicit destination!
```

Then upload the ZIP to the My Oracle Support SR and paste incident ID + problem key.

## Retention and Purging

`SET CONTROL` values are **hours**; `PURGE -AGE` is **minutes** — do not mix them up.

```bash
SHOW CONTROL                                    # LONGP (default 8760h = 1yr), SHORTP (720h = 30d)
SET CONTROL (SHORTP_POLICY=720)                 # 30 days
SET CONTROL (LONGP_POLICY=4320)                 # 180 days
PURGE                                           # apply retention policies now
PURGE -AGE 43200 -TYPE TRACE                    # traces older than 30 days
PURGE -I 12344                                  # one incident (never rm files from OS!)
```

Weekly cron purge example:

```bash
adrci exec="SET HOMEPATH diag/rdbms/orcl/orcl; PURGE -AGE 129600 -TYPE INCIDENT; PURGE -AGE 43200 -TYPE TRACE"
```

*No dedicated tool — run via `oracle_execute_sql`:* recent trace file inventory (11g+ ADR):

```sql
SELECT trace_filename, ROUND(file_size/1024,1) AS size_kb, change_time
FROM   v$diag_trace_file
ORDER  BY change_time DESC
FETCH FIRST 20 ROWS ONLY;
```

## Best Practices

1. Package diagnostics with IPS (`CORRELATE ALL`), never hand-zip trace files — manual zips miss metadata and correlated data.
2. Schedule weekly ADR purge; monitor `$ADR_HOME` disk usage — a recurring ORA-00600 flood can fill the file system in hours.
3. Use `SHOW ALERT -TAIL 20 -F` for live tailing; use predicates, not grep.
4. Never delete files under `$ADR_HOME/trace|incident` from the OS — it orphans ADR metadata; use `PURGE`.

## Common Mistakes

- **No `SET HOMEPATH`** with multiple homes — results silently span (or miss) homes. Set it explicitly every session.
- **`PURGE -AGE 30` purges 30 minutes**, not 30 days (that's 43200).
- **`IPS GENERATE` without `IN /path`** writes the ZIP to the current directory, which may be full or temp.
- **`SHOW ALERT` lags slightly** on very busy systems; for real time use `-TAIL -F`.

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance is valid for 19c unless a higher minimum is called out. For dual-support estates, verify defaults/deprecations per release update.

## See Also

- [Alert Log Analysis](alert-log-analysis.md) — interpreting ORA- entries
- [Health Monitor](health-monitor.md) — DBMS_HM checks behind `oracle_hm_findings`

## Sources

- [19c Administrator's Guide — Diagnosing and Resolving Problems](https://docs.oracle.com/en/database/oracle/oracle-database/19/admin/diagnosing-and-resolving-problems.html)
- [19c Utilities — ADRCI](https://docs.oracle.com/en/database/oracle/oracle-database/19/sutil/oracle-adr-command-interpreter-adrci.html)
- [19c Reference — V$DIAG_INFO](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-DIAG_INFO.html)
