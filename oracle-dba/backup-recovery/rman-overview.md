# Oracle RMAN Backup and Recovery

## Overview

Recovery Manager (RMAN) is Oracle's primary tool for database backup, restore, and recovery: block-level integrity checking, compression, encryption, incrementals, and tight integration with the database engine. Mastering backup and recovery is the single most critical DBA skill — a database that cannot be recovered cannot be trusted.

Architecture: the RMAN client drives **target database** server processes; backup metadata lives in the target's control file and optionally a **recovery catalog** (a separate schema enabling cross-database reporting, stored scripts, and longer history); an optional **media management layer** writes to tape via SBT channels; **channels** are the server processes performing the I/O.

## Backup Sets vs Image Copies

- **Backup set** — RMAN's proprietary format (one or more backup pieces); skips unused blocks; supports compression, encryption, and incrementals; required for tape; must be restored before use.
- **Image copy** — bit-for-bit copy of a datafile/archived log/control file; usable directly by Oracle (switch + recover, no restore step); larger on disk; incrementally updatable via `RECOVER COPY`.

| Factor | Backup Set | Image Copy |
|---|---|---|
| Space | Smaller (skips empty blocks) | Larger (full copy) |
| Recovery time | Restore + recover | Switch + recover |
| Tape support | Yes | No (directly) |

```sql
BACKUP DATABASE;                        -- backup set
BACKUP AS COMPRESSED BACKUPSET DATABASE;
BACKUP AS COPY DATABASE;                -- image copies
BACKUP AS COPY DATAFILE '/oradata/users01.dbf';
```

## Incremental Backups (Level 0 / Level 1)

Level 0 is the baseline (all used blocks); level 1 copies changed blocks — **differential** (since the last level 0 or 1; default) or **cumulative** (since the last level 0). Typical week: Sunday level 0, daily level 1.

**Block Change Tracking (Enterprise Edition)** avoids full datafile scans during incrementals:

```sql
ALTER DATABASE ENABLE BLOCK CHANGE TRACKING
  USING FILE '/oradata/bct/change_tracking.bct';
```

BCT status — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT status, filename FROM v$block_change_tracking;
```

**Incrementally updated backups** — a rolling image copy, always current to yesterday, giving very fast recovery:

```sql
-- Day 1: level 0 image-copy baseline
BACKUP INCREMENTAL LEVEL 0 AS COPY DATABASE TAG 'daily_copy';

-- Daily: roll the copy forward
RECOVER COPY OF DATABASE WITH TAG 'daily_copy' UNTIL TIME 'SYSDATE - 1';
BACKUP INCREMENTAL LEVEL 1 FOR RECOVER OF COPY WITH TAG 'daily_copy' DATABASE;
```

## Retention Policies

> **Operational — risk / confirm:** retention changes determine what `DELETE OBSOLETE` removes — align with the documented backup SLA before changing.

```sql
CONFIGURE RETENTION POLICY TO RECOVERY WINDOW OF 7 DAYS;   -- point-in-time window
CONFIGURE RETENTION POLICY TO REDUNDANCY 2;                -- fixed copy count
CONFIGURE RETENTION POLICY TO NONE;                        -- keep everything (avoid)

REPORT OBSOLETE;        -- preview what is deletable
DELETE OBSOLETE;        -- remove it
CROSSCHECK BACKUP;      -- reconcile missing pieces first
DELETE EXPIRED BACKUP;
```

## Recovery Catalog

Use a catalog for production databases: history beyond `CONTROL_FILE_RECORD_KEEP_TIME`, stored scripts, cross-database reporting, and virtual private catalogs for delegated access.

```sql
-- In the catalog database
CREATE TABLESPACE rman_cat
  DATAFILE '/oradata/rmancat/rman_cat01.dbf' SIZE 500M AUTOEXTEND ON;
CREATE USER rman_owner IDENTIFIED BY <password>
  DEFAULT TABLESPACE rman_cat QUOTA UNLIMITED ON rman_cat;
GRANT RECOVERY_CATALOG_OWNER TO rman_owner;
```

```bash
rman catalog rman_owner/<password>@catdb
RMAN> CREATE CATALOG;

rman target sys/<password>@proddb catalog rman_owner/<password>@catdb
RMAN> REGISTER DATABASE;
RMAN> RESYNC CATALOG;    -- synchronize with the target control file
```

## Recovery Scenarios

> **Operational — preconditions / risk / confirm:** every recovery below is production-impacting. Confirm the scenario (which files, which target time/SCN/sequence), take a fresh control file backup where possible, and get explicit approval before `OPEN RESETLOGS` — it resets the log sequence and creates a new incarnation.

Connect: `rman target /` (local OS auth), or with `target ... catalog ...`.

**Complete recovery** (no data loss; needs all archived logs from the backup through current SCN):

```sql
STARTUP MOUNT;
RESTORE DATABASE;
RECOVER DATABASE;
ALTER DATABASE OPEN;
```

**Point-in-time recovery** (undo an accidental drop or corruption; by SCN, time, or sequence):

```sql
STARTUP MOUNT;
RUN {
  SET UNTIL TIME "TO_DATE('2025-12-01 14:30:00','YYYY-MM-DD HH24:MI:SS')";
  RESTORE DATABASE;
  RECOVER DATABASE;
}
ALTER DATABASE OPEN RESETLOGS;
```

**Single datafile loss:**

```sql
ALTER DATABASE DATAFILE '/oradata/users01.dbf' OFFLINE;
RESTORE DATAFILE '/oradata/users01.dbf';
RECOVER DATAFILE '/oradata/users01.dbf';
ALTER DATABASE DATAFILE '/oradata/users01.dbf' ONLINE;
```

**Control file loss:**

```sql
STARTUP NOMOUNT;
RESTORE CONTROLFILE FROM AUTOBACKUP;
ALTER DATABASE MOUNT;
RECOVER DATABASE;
ALTER DATABASE OPEN RESETLOGS;
```

**TSPITR** — recover one tablespace to a past point while the rest of the database stays open (uses an auxiliary instance):

```sql
RECOVER TABLESPACE users UNTIL TIME 'SYSDATE - 2/24'
  AUXILIARY DESTINATION '/tmp/tspitr_aux';
```

## Best Practices

- **Control file autobackup ON** (default at `COMPATIBLE >= 12.2` — verify on older-compatibility databases), with a disk format.
- **Always back up archived logs**: `BACKUP DATABASE PLUS ARCHIVELOG DELETE INPUT;`
- **Test backups regularly**: `RESTORE DATABASE VALIDATE;`
- **Use a recovery catalog** for production; **enable BCT** on EE.
- **Store backups off-host** — on-host disk backups are useless if the server is lost (NFS, ASM, Object Storage, tape).
- **Monitor job history**: call `oracle_rman_backup_summary` → recent jobs with status, duration, bytes → alarm on failures.
- **Back up before and after significant changes** (migrations, patches, upgrades); rehearse recovery runbooks at least annually.

## Common Mistakes and How to Avoid Them

**Backing up to the same disk as the database** — one disk failure loses both; use a separate storage tier.

**Never testing restores** — untested backups are assumptions; schedule quarterly restore tests to a separate server.

**Ignoring backup job failures** — failed jobs often only surface in logs: call `oracle_rman_backup_summary` → check recent status → investigate failures the same day.

**No control file/SPFILE backup** — if autobackup is off, run `BACKUP CURRENT CONTROLFILE;` and `BACKUP SPFILE;` explicitly.

**Archived logs filling the FRA:** call `oracle_fra_usage` → limit vs used vs reclaimable → if near the limit, free space with RMAN or enlarge `db_recovery_file_dest_size`:

```sql
DELETE ARCHIVELOG ALL BACKED UP 1 TIMES TO DEVICE TYPE DISK;
```

**NOARCHIVELOG mode in production** — recoverable only to the last full backup; everything after is lost. Check via `oracle_execute_sql` (`SELECT log_mode FROM v$database;` — no dedicated tool), then enable:

```sql
SHUTDOWN IMMEDIATE;
STARTUP MOUNT;
ALTER DATABASE ARCHIVELOG;
ALTER DATABASE OPEN;
```

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance is valid for Oracle Database 19c. Autobackup defaults and BCT behavior differ by `COMPATIBLE` — verify per database.

## See Also

- [RMAN Basics: Commands, Configuration, and Operations](rman-basics.md) — day-to-day RMAN command reference

## Sources

- [Backup and Recovery User's Guide 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/bradv/)
- [RMAN Reference 19c — BACKUP](https://docs.oracle.com/en/database/oracle/oracle-database/19/rcmrf/BACKUP.html)
- [RMAN Reference 19c — CONFIGURE](https://docs.oracle.com/en/database/oracle/oracle-database/19/rcmrf/CONFIGURE.html)
- [Reference 19c — CONTROL_FILE_RECORD_KEEP_TIME](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/CONTROL_FILE_RECORD_KEEP_TIME.html)
