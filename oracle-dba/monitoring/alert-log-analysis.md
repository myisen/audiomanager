# Alert Log Analysis

The alert log is the primary diagnostic record of an instance: startups/shutdowns, administrative operations, structural changes and errors. It lives in the ADR (11g+) in two synchronized formats — XML (`$ADR_HOME/alert/log.xml`, authoritative, queried by `adrci` and `v$diag_alert_ext`) and text (`$ADR_HOME/trace/alert_<SID>.log`, human-readable).

Call `oracle_diag_info` → read `ADR Base`, `ADR Home`, `Diag Alert`, `Diag Trace` to locate everything. Pre-11g databases keep the log at `background_dump_dest` (check via `oracle_nondefault_parameters` or execute_sql).

## First Triage: Recent Errors

Call `oracle_alert_log_errors` (12.1+, param: minutes, default 120) → newest-first ORA- entries from the XML log. Follow with `oracle_active_alerts` for server-generated threshold alerts. For interactive filtering or live tailing use adrci (see [adrci-usage](adrci-usage.md)):

```bash
adrci> SHOW ALERT -P "MESSAGE_TEXT LIKE '%ORA-%' AND ORIGINATING_TIMESTAMP > TIMESTAMP '2026-03-06 00:00:00'"
```

## Common ORA- Errors and Where to Look Next

**ORA-00600 / ORA-07445 — internal errors.** Always capture the argument signature and trace file, open a Support SR. Repeated identical arguments = one bug. Check `oracle_hm_findings` for the automatic diagnostic run.

**ORA-01578 / ORA-01110 — block corruption.** Suspect hardware; run RMAN `VALIDATE DATABASE` (operational — needs approval). Call `oracle_block_corruption` → any row is a restore situation; pair with `oracle_rman_backup_summary`.

**ORA-04031 — shared pool exhausted.** Call `oracle_shared_pool_top` → look for runaway allocations; then `oracle_literal_sql` (literal SQL families) and `oracle_instance_load_stats` (hard parse ratio).

**ORA-00257 — archiver stuck.** Call `oracle_fra_usage` → `oracle_archived_log_inventory` → `oracle_archive_dest_status` (failed/deferred destination is the usual cause) → `oracle_alert_log_errors` for the underlying disk error.

**ORA-01555 — snapshot too old.** Call `oracle_undo_health` (ORA-01555 counter per 10-min window) → `oracle_long_transactions` (a forgotten open transaction is a common cause, not just undersized undo) → `oracle_tablespace_usage` for the undo tablespace row. Do not reflexively raise `UNDO_RETENTION`.

**ORA-01652 — temp exhausted.** Call `oracle_temp_usage` → `oracle_temp_by_session` (find the session).

**ORA-00060 — deadlock.** Oracle killed one statement; the trace file has the deadlock graph — always read it. Check `oracle_alert_log_errors` for frequency.

**ORA-03113/03114, ORA-12514/12541.** Crashed server process or listener trouble — match trace files in `Diag Trace`; `lsnrctl status` on the host.

Informational baseline: `Thread 1 advanced to log sequence` (switches — see below), `Completed checkpoint`, instance lifecycle, recorded DDL.

## Patterns to Watch

**Frequent log switches** (>1 per 15–20 min) → `oracle_redo_switches` + `oracle_redo_log_config` → small/undersized redo logs; fix by adding groups or resizing (operational change, needs approval).

**`Checkpoint not complete`** — the log writer stalls all DML until the checkpoint finishes. Fix: more/larger redo groups, tune `FAST_START_MTTR_TARGET` (operational).

**Growing incident count** — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT problem_key, COUNT(*) AS incident_count
FROM   v$diag_incident
GROUP  BY problem_key
ORDER  BY incident_count DESC;
```

## Automating Alert Log Monitoring

Offset-tracking shell poll (cron every 5–10 min, scans only new content):

```bash
#!/bin/bash
ALERT_LOG="/u01/app/oracle/diag/rdbms/orcl/orcl/trace/alert_orcl.log"
STATE_FILE="/tmp/alert_log_offset_orcl"
CURRENT_SIZE=$(wc -c < "$ALERT_LOG")
LAST_OFFSET=0; [ -f "$STATE_FILE" ] && LAST_OFFSET=$(cat "$STATE_FILE")
if [ "$CURRENT_SIZE" -gt "$LAST_OFFSET" ]; then
    NEW_ERRORS=$(tail -c +$((LAST_OFFSET + 1)) "$ALERT_LOG" |
                 grep -E "ORA-[0-9]+" | grep -v "ORA-00000")
    [ -n "$NEW_ERRORS" ] && echo "$NEW_ERRORS" | mail -s "Oracle Alert: orcl errors" dba@example.com
    echo "$CURRENT_SIZE" > "$STATE_FILE"
fi
```

From SQL, any scheduler can poll the same window — that is exactly what `oracle_alert_log_errors` does; wire it into a recurring health check with a 10–15 minute window and watch for `checkpoint not complete` text as well as ORA-.

## Best Practices

1. **Never delete/truncate the alert log** — Oracle rotates it; prune via `adrci PURGE`.
2. **Monitor continuously** (5–10 min poll). Near-real-time alerting for ORA-00600/07445/01578.
3. **Read the referenced trace file first** — it has the full context before you search MOS.
4. **Correlate timestamps with OS metrics** to separate bugs from resource pressure.
5. **Know your baseline**: normal switch frequency, recurring warnings. Deviation is the first signal.
6. **Record alert-log state before maintenance windows** and review everything written during the window.

## Common Mistakes

- **Ignoring ORA-00600/07445 because the DB "seems fine"** — they always mean something unexpected; capture and file an SR.
- **Grepping only `ORA-`** — `checkpoint not complete`, `WARNING: Heavy swapping`, `db_block_checking` errors matter too.
- **Treating every ORA-01555 as a tuning problem** — check for application-held open transactions first.
- **Assuming XML and text logs are always in sync** — after crashes they can diverge; XML is authoritative.
- **Monitoring across a stale NFS mount** — the poll silently stops seeing new entries.

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c unless a higher minimum is noted. `oracle_alert_log_errors` requires 12.1+; on 10g/11g poll the text log via script or adrci.

## See Also

- [ADRCI Usage](adrci-usage.md) — filtering, incidents, IPS packaging
- [Health Monitor](health-monitor.md) — ADR health checks

## Sources

- [19c Administrator's Guide — Diagnosing and Resolving Problems](https://docs.oracle.com/en/database/oracle/oracle-database/19/admin/diagnosing-and-resolving-problems.html)
- [19c Reference — V$DIAG_ALERT_EXT](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-DIAG_ALERT_EXT.html)
