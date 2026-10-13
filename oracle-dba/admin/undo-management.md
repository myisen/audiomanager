# Oracle Undo Management

## Overview

Undo stores the "before image" of data modified by uncommitted transactions. It enables transaction rollback, read consistency (queries reconstruct a snapshot from undo), and the Flashback features. Modern databases use **Automatic Undo Management (AUM)** — a dedicated, automatically managed undo tablespace; manual rollback segments are deprecated and must not be used.

Key parameters: `UNDO_MANAGEMENT=AUTO` (default since 10g), `UNDO_TABLESPACE` (one active per instance/thread), `UNDO_RETENTION` (low-threshold retention target in seconds, default 900 — Oracle auto-tunes above it when space allows), and `RETENTION GUARANTEE` on the undo tablespace (never overwrite unexpired undo; shifts the failure mode from ORA-01555 to ORA-30036).

---

## Health Checks (tool-driven)

- Call `oracle_undo_health` → check `SSOLDERRCNT` (ORA-01555 errors), `NOSPACEERRCNT` (ORA-30036 errors), `MAXQUERYLEN` (longest query), undo generation rate, and tuning status. Any non-zero error counter needs follow-up.
- Call `oracle_long_transactions` → transactions holding undo for hours; the usual root cause of both 01555 and sudden undo growth. Next step: confirm their SQL with `oracle_sql_text`.
- Call `oracle_tablespace_usage` on the undo tablespaces → used percent, CRITICAL/WARNING grading, and autoextend headroom.
- Call `oracle_nondefault_parameters` → `undo_management`, `undo_tablespace`, `undo_retention` (if absent, the parameter is at its default).
- Call `oracle_datafile_usage` on undo datafiles → autoextend enabled and MAXSIZE cap.

Reference — undo extent states (`DBA_UNDO_EXTENTS.STATUS`): `ACTIVE` = held by an uncommitted transaction; `UNEXPIRED` = committed but still within the retention window; `EXPIRED` = reclaimable. Undo consumed by ACTIVE transactions is never reclaimed regardless of retention settings.

---

## Parameter and Tablespace Changes

**Risk:** `RETENTION GUARANTEE` can make active transactions fail with ORA-30036 when the tablespace is full — only enable with sufficient headroom. `UNDO_RETENTION` below your longest query duration invites ORA-01555.

```sql
-- Switch the active undo tablespace
ALTER SYSTEM SET UNDO_TABLESPACE = undotbs2 SCOPE=BOTH;

-- Raise retention (e.g., to 1 hour)
ALTER SYSTEM SET UNDO_RETENTION = 3600 SCOPE=BOTH;

-- Enable/disable retention guarantee
ALTER TABLESPACE undotbs1 RETENTION GUARANTEE;
ALTER TABLESPACE undotbs1 RETENTION NOGUARANTEE;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT tablespace_name, retention FROM dba_tablespaces
WHERE contents = 'UNDO';
```

---

## Sizing and Creating the Undo Tablespace

Sizing inputs come from `oracle_undo_health` (generation rate, `MAXQUERYLEN`, peak concurrency). Size for **peak**, not average — month-end batch can generate 10–100x normal undo. Formula: retention seconds × block size × undo blocks/sec + overhead for active transactions.

**Preconditions:** new datafile path exists with space; AUTOEXTEND capped with MAXSIZE.
**Risk:** the old undo tablespace cannot be dropped until its ACTIVE/UNEXPIRED extents age out.

```sql
-- Create a new undo tablespace with capped autoextend
CREATE UNDO TABLESPACE undotbs2
  DATAFILE '/oradata/undotbs2_01.dbf' SIZE 2G
  AUTOEXTEND ON NEXT 512M MAXSIZE 20G;

-- Add space to an existing undo tablespace
ALTER TABLESPACE undotbs1
  ADD DATAFILE '/oradata/undotbs1_02.dbf' SIZE 2G AUTOEXTEND ON MAXSIZE 20G;

-- Switch, then drop the old one only after all transactions migrate
ALTER SYSTEM SET UNDO_TABLESPACE = undotbs2 SCOPE=BOTH;
DROP TABLESPACE undotbs1 INCLUDING CONTENTS AND DATAFILES;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
-- Verify the old tablespace is empty before dropping
SELECT status, COUNT(*) FROM dba_undo_extents
WHERE tablespace_name = 'UNDOTBS_OLD'
GROUP BY status;
-- ACTIVE/UNEXPIRED rows = not yet safe to drop
```

---

## ORA-01555: Snapshot Too Old

**Cause:** a long-running query cannot reconstruct its read-consistent snapshot because the undo it needs was overwritten. Typical scenarios: long batch queries over hot tables, slowly fetched cursors (e.g., JDBC fetching row-by-row), undersized undo with no RETENTION GUARANTEE, very low `UNDO_RETENTION`.

**Fixes:**
1. Raise `UNDO_RETENTION` to cover the longest query (see command above).
2. Add undo space — `oracle_undo_health` → `MAXQUERYLEN` tells you the minimum retention window.
3. Enable RETENTION GUARANTEE only if ORA-30036 risk is acceptable.
4. Fix applications: use larger fetch sizes, close cursors promptly; prefer Flashback Query (`AS OF`) over holding cursors open.
5. Schedule heavy reports away from heavy DML; use parallel query to shorten scan windows.

## Undo Advisor (DBMS_UNDO_ADV)

Read-only advisor calls; safe to run anytime. The same analysis is available graphically in Enterprise Manager under **Advisor Central → Undo Advisor**.

```sql
-- Recommended retention based on the last 7 days
SELECT DBMS_UNDO_ADV.REQUIRED_RETENTION(
         start_time => SYSDATE - 7, end_time => SYSDATE
       ) AS recommended_retention_seconds
FROM DUAL;

-- Minimum tablespace size for a target retention
SELECT DBMS_UNDO_ADV.REQUIRED_UNDO_SIZE(
         retention => 3600, start_time => SYSDATE - 7, end_time => SYSDATE
       ) AS required_mb
FROM DUAL;
```

---

## Best Practices

- Always use AUM (`UNDO_MANAGEMENT=AUTO`).
- Enable AUTOEXTEND on undo datafiles but cap MAXSIZE.
- Set `UNDO_RETENTION` from your longest expected query runtime, measured via `oracle_undo_health` (`MAXQUERYLEN`), not a fixed number.
- Run the Undo Advisor before resizing or tuning.
- Monitor `SSOLDERRCNT` and `NOSPACEERRCNT` daily — non-zero means users are hitting ORA-01555 / ORA-30036.
- Do not enable RETENTION GUARANTEE unless the tablespace absorbs peak workloads.
- Size for peak (month-end, ETL windows), not average.

---

## Common Mistakes

**Default UNDO_RETENTION=900 with long batch reports** — any query over 15 minutes is a 01555 candidate.

**AUTOEXTEND without MAXSIZE** — a runaway UPDATE consumes all disk. Always cap.

**Dropping the old undo tablespace immediately after switching** — it cannot drop until all ACTIVE/UNEXPIRED undo ages out; verify with the `dba_undo_extents` check above.

**Ignoring NOSPACEERRCNT** — transactions are failing with ORA-30036; the tablespace is too small or GUARANTEE is on without space.

**Manually dropping/altering undo segments** — never interfere with AUM-managed segments; it causes corruption.

---

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance valid for Oracle Database 19c unless a newer minimum is explicitly stated.
- Features marked 21c/23c/23ai should be treated as 26ai-capable; keep 19c-compatible alternatives for mixed-version estates.
- In dual-support environments, test syntax and behavior on both 19c and 26ai; defaults and deprecations differ by release update.

## Sources

- [Oracle Database Administrator's Guide 19c — Managing Undo](https://docs.oracle.com/en/database/oracle/oracle-database/19/admin/managing-undo.html)
- [Oracle 19c Reference — V$UNDOSTAT](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-UNDOSTAT.html)
- [Oracle 19c Reference — DBA_UNDO_EXTENTS](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/DBA_UNDO_EXTENTS.html)
