# RMAN Basics: Commands, Configuration, and Operations

## Overview

RMAN (Recovery Manager) is Oracle's built-in backup and recovery tool. Unlike OS-level file copies, it understands Oracle's block structure, validates blocks for corruption, skips unused space, performs incremental backups, and integrates with the media management layer for tape. This is the day-to-day command reference; for architecture, retention strategy, and recovery scenarios see `rman-overview.md`.

Before any RMAN work, check the backup state from SQL: call `oracle_rman_backup_summary` → recent backup jobs with status, timing, and input/output bytes → confirm the last full/incremental succeeded.

## Connecting to RMAN

RMAN connects to the target database as a privileged user (`SYSDBA` or `SYSBACKUP`), optionally with a recovery catalog or an auxiliary instance.

```bash
rman target /                                                  # local OS auth
rman target sys/<password>@hostname:1521/service_name          # Easy Connect
rman target sys/<password>@proddb catalog rman_owner/<password>@catdb
rman target sys/<password>@proddb auxiliary sys/<password>@auxdb   # duplicate/TSPITR
```

Verify connectivity and repository:

```sql
RMAN> SHOW ALL;       -- persistent configuration settings
RMAN> LIST BACKUP;    -- catalog/control file reachable
```

## Channel Configuration

Channels are server processes doing backup/restore I/O — configured persistently, or allocated manually per `RUN` block.

```sql
-- Persistent configuration
CONFIGURE CHANNEL DEVICE TYPE DISK FORMAT '/backup/rman/%d_%U';
CONFIGURE DEVICE TYPE DISK PARALLELISM 2;
CONFIGURE CHANNEL DEVICE TYPE SBT
  PARMS 'SBT_LIBRARY=/opt/netbackup/lib/libobk.so,
         ENV=(NB_ORA_SERV=nbmaster,NB_ORA_POLICY=oracle_policy)';
CONFIGURE DEFAULT DEVICE TYPE TO DISK;
CONFIGURE CHANNEL DEVICE TYPE DISK CLEAR;   -- back to default

-- Manual allocation overrides automatic channels for this RUN block only
RUN {
  ALLOCATE CHANNEL c1 DEVICE TYPE DISK FORMAT '/backup/rman/ch1_%U';
  ALLOCATE CHANNEL c2 DEVICE TYPE DISK FORMAT '/backup/rman/ch2_%U';
  BACKUP INCREMENTAL LEVEL 0 DATABASE;
  RELEASE CHANNEL c1;
  RELEASE CHANNEL c2;
}
```

## BACKUP

```sql
BACKUP DATABASE;                                    -- full backup set
BACKUP DATABASE PLUS ARCHIVELOG DELETE INPUT;       -- + logs, delete after backup
BACKUP AS COMPRESSED BACKUPSET DATABASE;            -- compressed backup set
BACKUP AS COPY DATABASE;                            -- image copy
BACKUP INCREMENTAL LEVEL 0 DATABASE;                -- incremental baseline
BACKUP INCREMENTAL LEVEL 1 DATABASE;                -- differential (default)
BACKUP INCREMENTAL LEVEL 1 CUMULATIVE DATABASE;     -- since last level 0

BACKUP TABLESPACE users, example;
BACKUP DATAFILE 5;
BACKUP ARCHIVELOG ALL DELETE INPUT;
BACKUP ARCHIVELOG FROM SEQUENCE 500 UNTIL SEQUENCE 600 THREAD 1;

BACKUP CURRENT CONTROLFILE;
BACKUP SPFILE;
CONFIGURE CONTROLFILE AUTOBACKUP ON;
CONFIGURE CONTROLFILE AUTOBACKUP FORMAT FOR DEVICE TYPE DISK TO '/backup/cf_%F';

BACKUP DATABASE TAG 'full_before_upgrade';          -- tags ease identification at restore
```

## RESTORE

`RESTORE` copies backup pieces/image copies back; applying redo is `RECOVER`'s job.

```sql
STARTUP MOUNT;
RESTORE DATABASE;                                   -- database must be mounted
RESTORE TABLESPACE users;
RESTORE DATAFILE 5;

STARTUP NOMOUNT;
RESTORE CONTROLFILE FROM AUTOBACKUP;
RESTORE CONTROLFILE FROM '/backup/rman/cf_c-12345-20251201-00';

-- Restore to different locations (relocation/testing)
RUN {
  SET NEWNAME FOR DATAFILE '/oradata/users01.dbf' TO '/oradata_new/users01.dbf';
  RESTORE DATABASE;
  SWITCH DATAFILE ALL;
}

RESTORE DATABASE PREVIEW SUMMARY;   -- what would be used; restores nothing
```

## RECOVER

`RECOVER` applies archived redo logs and incrementals to bring restored files to a consistent state.

```sql
RECOVER DATABASE;               -- complete recovery through current SCN

-- Point-in-time: set the endpoint once; RESTORE and RECOVER both use it
RUN {
  SET UNTIL TIME "TO_DATE('2025-12-01 14:30','YYYY-MM-DD HH24:MI')";
  RESTORE DATABASE;
  RECOVER DATABASE;
}
ALTER DATABASE OPEN RESETLOGS;  -- required after incomplete recovery

RECOVER TABLESPACE users;
RECOVER DATAFILE '/oradata/users01.dbf';
```

## CROSSCHECK and DELETE

> **Operational — risk / confirm:** DELETE commands remove physical backup pieces and repository records. Always `CROSSCHECK` first, preview with `REPORT OBSOLETE`, and confirm the retention SLA with the backup owner before deleting.

```sql
CROSSCHECK BACKUP;               -- mark missing pieces EXPIRED
CROSSCHECK COPY;
CROSSCHECK ARCHIVELOG ALL;

DELETE EXPIRED BACKUP;           -- drop repository entries for missing pieces
DELETE OBSOLETE;                 -- remove per retention policy
DELETE ARCHIVELOG ALL BACKED UP 1 TIMES TO DEVICE TYPE DISK;   -- safe log cleanup
DELETE ARCHIVELOG ALL BACKED UP 2 TIMES;                       -- safer: keep 2 copies

REPORT OBSOLETE;                 -- preview only, deletes nothing
```

Never delete backup files with OS commands — the repository desyncs; use `CROSSCHECK` + `DELETE EXPIRED BACKUP`.

## Reporting

```sql
LIST BACKUP SUMMARY;
LIST BACKUP OF DATABASE;
LIST BACKUP COMPLETED AFTER 'SYSDATE-2';
LIST BACKUP TAG 'full_before_upgrade';
LIST COPY OF DATABASE;

REPORT SCHEMA;                 -- datafiles, sizes
REPORT NEED BACKUP DAYS 7;     -- files at risk of insufficient backup
REPORT UNRECOVERABLE;          -- files with no sufficient backup
REPORT OBSOLETE;

SHOW ALL;
SHOW RETENTION POLICY;
SHOW CONTROLFILE AUTOBACKUP;
SHOW ENCRYPTION FOR DATABASE;
```

## Compression

```sql
CONFIGURE COMPRESSION ALGORITHM 'BASIC';    -- no additional license
CONFIGURE COMPRESSION ALGORITHM 'MEDIUM';   -- Advanced Compression option
BACKUP AS COMPRESSED BACKUPSET DATABASE;
```

Compression ratio of recent backups — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT input_bytes_display, output_bytes_display, compression_ratio
FROM   v$backup_set_details
ORDER  BY completion_time DESC
FETCH  FIRST 5 ROWS ONLY;
```

## Encryption

> **Operational — preconditions / risk / confirm:** transparent (keystore-based) backup encryption requires Advanced Security and an open keystore; password-based encryption needs no license, but a lost passphrase means an unrestorable backup — store it in the secrets vault.

```sql
CONFIGURE ENCRYPTION FOR DATABASE ON;       -- transparent, uses keystore/wallet
CONFIGURE ENCRYPTION ALGORITHM 'AES256';    -- default is AES128

SET ENCRYPTION ON IDENTIFIED BY <backup_password> ONLY;   -- password-only
BACKUP DATABASE;

-- Transparent encryption needs the keystore open first (12c+ syntax):
ADMINISTER KEY MANAGEMENT SET KEYSTORE OPEN IDENTIFIED BY <wallet_password>;
```

## Parallelism

```sql
CONFIGURE DEVICE TYPE DISK PARALLELISM 4;   -- match storage I/O capacity
CONFIGURE DEVICE TYPE SBT PARALLELISM 2;

RUN {
  ALLOCATE CHANNEL c1 DEVICE TYPE DISK FORMAT '/backup/%U';
  ALLOCATE CHANNEL c2 DEVICE TYPE DISK FORMAT '/backup/%U';
  ALLOCATE CHANNEL c3 DEVICE TYPE DISK FORMAT '/backup/%U';
  ALLOCATE CHANNEL c4 DEVICE TYPE DISK FORMAT '/backup/%U';
  BACKUP DATABASE;
}
```

Each channel writes a separate backup piece; more channels than the storage can handle causes contention.

## Operational Patterns

Full production weekly backup:

```sql
RUN {
  CONFIGURE RETENTION POLICY TO RECOVERY WINDOW OF 7 DAYS;
  CONFIGURE CONTROLFILE AUTOBACKUP ON;
  CONFIGURE COMPRESSION ALGORITHM 'BASIC';
  CONFIGURE DEVICE TYPE DISK PARALLELISM 4;
  CONFIGURE CHANNEL DEVICE TYPE DISK FORMAT '/backup/rman/%d_%T_%U';

  BACKUP AS COMPRESSED BACKUPSET
    INCREMENTAL LEVEL 0
    DATABASE
    INCLUDE CURRENT CONTROLFILE
    TAG 'weekly_full';

  BACKUP ARCHIVELOG ALL DELETE INPUT TAG 'archlog_weekly';
  DELETE NOPROMPT OBSOLETE;
}
```

Validate recoverability without restoring (schedule periodically in a maintenance window):

```sql
RESTORE DATABASE VALIDATE;                  -- physical integrity + piece availability
RESTORE DATABASE VALIDATE CHECK LOGICAL;    -- + logical corruption checks
VALIDATE BACKUPSET 45;
```

Backup job monitoring: call `oracle_rman_backup_summary` → recent jobs with status, duration, input/output bytes → alarm on `FAILED` status and on jobs whose duration doubles. Currently running backups — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT sid, serial#, sofar, totalwork,
       ROUND(sofar/totalwork*100, 2) pct_done
FROM   v$session_longops
WHERE  opname LIKE 'RMAN%' AND opname NOT LIKE '%aggregate%'
AND    totalwork != 0;
```

## Best Practices

- `CROSSCHECK BACKUP` before `DELETE OBSOLETE`, always.
- `BACKUP DATABASE PLUS ARCHIVELOG DELETE INPUT` keeps archived logs from accumulating.
- `CONTROLFILE AUTOBACKUP ON` unconditionally.
- Tag every backup.
- Schedule `REPORT NEED BACKUP` as monitoring to catch datafiles that missed a cycle.
- Match parallelism to storage I/O capacity.
- Run `RESTORE DATABASE VALIDATE` periodically to prove recoverability.

## Common Mistakes and How to Avoid Them

**Running RMAN without privileges** — RMAN requires `SYSDBA` or `SYSBACKUP` on the target.

**Archivelog backups without `DELETE INPUT`** — logs accumulate; `DELETE ALL INPUT` (deletes regardless of backup status) is riskier than the `BACKED UP n TIMES` semantics.

**Database backups without archived logs** — not recoverable to a consistent state; always `PLUS ARCHIVELOG`.

**`DELETE ARCHIVELOG ALL`** — deletes logs even if never backed up; use `DELETE ARCHIVELOG ALL BACKED UP 2 TIMES`.

**VALIDATE vs RESTORE VALIDATE** — `VALIDATE BACKUPSET` checks pieces are readable; only `RESTORE DATABASE VALIDATE` proves a restore is actually feasible.

**OS-deleting backup files** — repository desyncs; `CROSSCHECK` + `DELETE EXPIRED BACKUP` instead.

**`CONTROL_FILE_RECORD_KEEP_TIME` too small** — without a catalog, control-file backup records age out (default 7 days); raise it to match retention:

```sql
ALTER SYSTEM SET control_file_record_keep_time = 31 SCOPE=BOTH;
```

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance is valid for Oracle Database 19c. Control file autobackup defaults ON at `COMPATIBLE >= 12.2` — verify with `SHOW CONTROLFILE AUTOBACKUP` on older-compatibility databases.

## See Also

- [Oracle RMAN Backup and Recovery](rman-overview.md) — RMAN architecture, backup strategy, and recovery scenarios

## Sources

- [Backup and Recovery User's Guide 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/bradv/)
- [RMAN Reference 19c — BACKUP](https://docs.oracle.com/en/database/oracle/oracle-database/19/rcmrf/BACKUP.html)
- [RMAN Reference 19c — CONFIGURE](https://docs.oracle.com/en/database/oracle/oracle-database/19/rcmrf/CONFIGURE.html)
- [RMAN Reference 19c — RESTORE](https://docs.oracle.com/en/database/oracle/oracle-database/19/rcmrf/RESTORE.html)
- [RMAN Reference 19c — RECOVER](https://docs.oracle.com/en/database/oracle/oracle-database/19/rcmrf/RECOVER.html)
