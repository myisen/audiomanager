# Oracle Redo Log Management

## Overview

The redo log is Oracle's write-ahead log: every change is recorded as a redo entry in the log buffer, then flushed by LGWR into the **online redo log files**. This guarantees durability — after a crash, redo replays all committed changes since the last checkpoint. Undersized redo causes excessive log switches, checkpoint pressure, and throughput degradation; a full archive destination or FRA can hang the database entirely.

Redo is organized in **groups** (minimum 2; use 3+ in production), each with one or more **members** (multiplexed copies on separate disks). Exactly one group is CURRENT at a time. Group statuses: `CURRENT` (LGWR writing), `ACTIVE` (needed for instance recovery), `INACTIVE` (reusable), `UNUSED`.

---

## Inspecting the Current Configuration

- Call `oracle_redo_log_config` → per group: size, member count, member status, archived flag. All members should be VALID — INVALID/STALE means a lost member needing re-add.
- Call `oracle_redo_switches` → switches per hour/day. Sustained switches more often than every ~15–30 minutes at peak means undersized logs.
- Call `oracle_fra_usage` → FRA percent used; a full FRA hangs the database on the next switch (act above ~80%).
- Call `oracle_archived_log_inventory` → archived log stock on disk and oldest/newest timestamps.
- Call `oracle_wait_events` → filter events like `log file switch (checkpoint incomplete)` / `(archiving needed)`; follow up with `oracle_session_wait_details` for sessions stuck on `log file sync`.

Wait-event decoding: `checkpoint incomplete` → add groups or speed checkpoints; `archiving needed` → ARCn cannot keep up; occasional `log file switch completion` is normal.

---

## Adding, Dropping, Resizing

**Preconditions:** member paths exist on separate disks/controllers; confirm current group statuses with `oracle_redo_log_config` first.
**Risk:** groups can only be dropped when INACTIVE, and never below 2 groups total.

```sql
-- Add a new group with two members (multiplexed)
ALTER DATABASE ADD LOGFILE GROUP 4
  ('/disk1/redo04a.log', '/disk2/redo04b.log') SIZE 500M;

-- Add a member to an existing group
ALTER DATABASE ADD LOGFILE MEMBER '/disk2/redo01b.log' TO GROUP 1;

-- Drop: only INACTIVE groups; never drop below 2 groups
ALTER DATABASE DROP LOGFILE GROUP 4;
ALTER DATABASE DROP LOGFILE MEMBER '/disk2/redo01b.log';
-- The OS file of a dropped member is not removed automatically.
```

### Resizing (no ALTER RESIZE — rebuild instead)

**Risk:** forced switches/checkpoints add I/O — do it in a quiet window; verify with `oracle_redo_log_config` at the end.

```sql
-- 1. Add new groups at the desired size
ALTER DATABASE ADD LOGFILE GROUP 4 ('/oradata/redo04a.log') SIZE 1G;
ALTER DATABASE ADD LOGFILE GROUP 5 ('/oradata/redo05a.log') SIZE 1G;
ALTER DATABASE ADD LOGFILE GROUP 6 ('/oradata/redo06a.log') SIZE 1G;

-- 2. Cycle old groups to INACTIVE
ALTER SYSTEM SWITCH LOGFILE;   -- repeat until old groups are INACTIVE
ALTER SYSTEM CHECKPOINT;       -- advances checkpoint so ACTIVE → INACTIVE

-- 3. Drop the old undersized groups
ALTER DATABASE DROP LOGFILE GROUP 1;
ALTER DATABASE DROP LOGFILE GROUP 2;
ALTER DATABASE DROP LOGFILE GROUP 3;
```

### Sizing Rule of Thumb

Size logs so a switch occurs every 15–30 minutes at peak. Example: 200 MB logs switching every 3 minutes → ~66 MB/min → target ≈ 1.3 GB per log for a 20-minute cycle.

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
-- Average redo per log over the last 7 days (input for sizing)
SELECT ROUND(AVG(blocks * block_size) / 1048576, 1) avg_mb_per_log
FROM v$archived_log
WHERE first_time > SYSDATE - 7 AND standby_dest = 'NO';

-- Cumulative redo rate and space waits
SELECT statistic#, name, value FROM v$sysstat
WHERE name IN ('redo size', 'redo entries', 'redo log space requests',
               'redo log space wait time');
```

Non-zero `redo log space requests` / `redo log space wait time` confirms LGWR is stalling on log switches.

---

## ARCHIVELOG Mode

In ARCHIVELOG mode, ARCn copies each log group to the archive destination before reuse, enabling point-in-time recovery, hot backups, and Data Guard. In NOARCHIVELOG mode, recovery is limited to the last full backup — production must run ARCHIVELOG.

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT log_mode FROM v$database;
```

**Enabling requires downtime; afterwards a full backup must be taken immediately (prior backups become unusable for media recovery). Requires explicit human confirmation.**
```sql
SHUTDOWN IMMEDIATE;
STARTUP MOUNT;
ALTER DATABASE ARCHIVELOG;
ALTER DATABASE OPEN;
```

### Archive Destinations

**Preconditions:** the destination path exists with adequate space (or use the FRA).
**Verify after changes:** `oracle_archive_dest_status` → `status` = `VALID`, no `error`; `oracle_fra_usage` if using the FRA.

```sql
ALTER SYSTEM SET LOG_ARCHIVE_DEST_1 =
  'LOCATION=/oradata/archive VALID_FOR=(ALL_LOGFILES,ALL_ROLES)' SCOPE=BOTH;
ALTER SYSTEM SET LOG_ARCHIVE_DEST_STATE_1 = ENABLE SCOPE=BOTH;

-- Or use the Fast Recovery Area as the archive destination
ALTER SYSTEM SET DB_RECOVERY_FILE_DEST = '/oradata/fra' SCOPE=BOTH;
ALTER SYSTEM SET DB_RECOVERY_FILE_DEST_SIZE = 100G SCOPE=BOTH;
ALTER SYSTEM SET LOG_ARCHIVE_DEST_1 =
  'LOCATION=USE_DB_RECOVERY_FILE_DEST' SCOPE=BOTH;
```

### Manual Switch / Archive

```sql
ALTER SYSTEM SWITCH LOGFILE;
ALTER SYSTEM ARCHIVE LOG CURRENT;
```

---

## Archived Log Retention (RMAN Only)

**Risk:** deleting logs that were never backed up breaks point-in-time recovery; never delete via OS commands. Confirm contents with `oracle_archived_log_inventory` and backup coverage with `oracle_rman_backup_summary`. **Requires explicit confirmation.**

```sql
-- RMAN: delete archived logs already backed up at least once
DELETE ARCHIVELOG ALL BACKED UP 1 TIMES TO DEVICE TYPE DISK;

-- RMAN: after accidental OS-level deletion — resync the repository
CROSSCHECK ARCHIVELOG ALL;
DELETE EXPIRED ARCHIVELOG ALL;
```

---

## Recovering from Log Member / Group Loss

### Missing member (other members intact)

Detect first: `oracle_redo_log_config` shows the member as INVALID/STALE while the group is CURRENT/ACTIVE.

```sql
ALTER DATABASE DROP LOGFILE MEMBER '/disk2/redo01b.log';
ALTER DATABASE ADD LOGFILE MEMBER '/disk3/redo01b.log' TO GROUP 1;
-- New member is INVALID until the next log switch
```

### Lost CURRENT group (media failure) — DATA LOSS SCENARIO

Only when the log truly cannot be recovered: unarchived redo is lost and media recovery may become impossible without a backup. **Requires explicit human sign-off.**

```sql
ALTER DATABASE CLEAR UNARCHIVED LOGFILE GROUP 1;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT file#, name, status FROM v$datafile WHERE status != 'ONLINE';
```

---

## Best Practices

- Production must run in ARCHIVELOG mode; NOARCHIVELOG means no point-in-time recovery.
- Multiplex redo logs across separate disks; keep LGWR storage low-latency (SSD or ASM).
- Size for 15–30 minute switches at peak; use 3–5 groups minimum, 6+ under bulk load.
- Store archived logs in the FRA; monitor `oracle_fra_usage` daily — a full FRA hangs the database.
- Never delete archived logs outside RMAN.
- Add groups before you need them; watch `oracle_wait_events` for `log file switch*` events.

---

## Common Mistakes

**Only 2 redo log groups** — LGWR stalls when the next group is still being archived. Use 3+.

**Unmirrored CURRENT log** — a single disk failure is unrecoverable unless you accept `CLEAR UNARCHIVED LOGFILE` data loss.

**Redo on the same disks as datafiles** — LGWR I/O competes with DBWR; commit latency suffers.

**Not watching FRA space** — at 100% the database hangs on the next switch. `oracle_fra_usage` → act above ~80%.

**ARCHIVELOG without a configured destination** — archives go to a default OS path that may lack space or backups.

**Dropping an ACTIVE group** — fails with ORA-00350; run `ALTER SYSTEM CHECKPOINT` first, then retry.

---

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance valid for Oracle Database 19c unless a newer minimum is explicitly stated.
- Features marked 21c/23c/23ai should be treated as 26ai-capable; keep 19c-compatible alternatives for mixed-version estates.
- In dual-support environments, test syntax and behavior on both 19c and 26ai; defaults and deprecations differ by release update.

## Sources

- [Oracle Database Administrator's Guide 19c — Managing the Redo Log](https://docs.oracle.com/en/database/oracle/oracle-database/19/admin/managing-the-redo-log.html)
- [Oracle 19c Reference — V$LOG](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-LOG.html)
- [Oracle 19c Reference — V$LOGFILE](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-LOGFILE.html)
- [Oracle 19c Reference — V$LOG_HISTORY](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-LOG_HISTORY.html)
