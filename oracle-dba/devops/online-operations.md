# Online Operations in Oracle DB

## Overview

Oracle lets structural changes run while the database keeps serving application traffic. The key mechanisms:

- **DBMS_REDEFINITION** — online table restructuring (type changes, column reordering, partitioning, compression, tablespace moves)
- **Online index operations** — create or rebuild indexes without blocking DML
- **ALTER TABLE ... ONLINE** — add columns and modify constraints with reduced locking
- **Online segment shrink** — reclaim space without taking tables offline

None is instantaneous on large objects, and each has prerequisites and edge cases. Diagnostic SQL is replaced below with tool guidance where a dedicated tool exists; operational SQL is kept verbatim and requires review plus human confirmation before production execution.

---

## DBMS_REDEFINITION

Restructures a table while it stays fully available for DML: create an interim table, start synchronization (an MV log tracks changes), copy rows, sync incrementally, then atomically swap names. Applications see no interruption; the final swap takes a brief exclusive lock (typically milliseconds). **Use for:** column type changes, column reordering, heap-to-partitioned conversion, compression changes, online tablespace moves — anything `ALTER TABLE` cannot handle.

**Preconditions / risks / human confirmation required:** table needs a PK (or `CONS_USE_ROWID`); owner needs `EXECUTE` on `DBMS_REDEFINITION`; ~1x the table size in extra storage — confirm free space first with `oracle_tablespace_usage` / `oracle_datafile_usage`; get explicit approval for the cutover step in production.

```sql
-- (1) Verify eligibility; if no exception is raised, the table is eligible
BEGIN
  DBMS_REDEFINITION.CAN_REDEF_TABLE(
    uname        => 'APP_OWNER',
    tname        => 'ORDERS',
    options_flag => DBMS_REDEFINITION.CONS_USE_PK  -- or CONS_USE_ROWID
  );
END;
/

-- (2) After creating the interim table ORDERS_NEW with the target structure
-- (here: range-partitioned by ORDER_DATE with monthly intervals, PK on
-- (ORDER_ID, ORDER_DATE); extra columns start as NULL), start the redefinition:
-- this creates the MV log on ORDERS and begins the copy
BEGIN
  DBMS_REDEFINITION.START_REDEF_TABLE(
    uname        => 'APP_OWNER',
    orig_table   => 'ORDERS',
    int_table    => 'ORDERS_NEW',
    col_mapping  => 'ORDER_ID ORDER_ID, CUSTOMER_ID CUSTOMER_ID, ' ||
                    'ORDER_DATE ORDER_DATE, STATUS_CODE STATUS_CODE, ' ||
                    'TOTAL_AMOUNT TOTAL_AMOUNT, SYSTIMESTAMP CREATED_AT',
    options_flag => DBMS_REDEFINITION.CONS_USE_PK
  );
END;
/

-- (3) Synchronize incrementally, one or more times during the copy;
-- applies DML since the last sync, reducing final cutover time
BEGIN
  DBMS_REDEFINITION.SYNC_INTERIM_TABLE(
    uname      => 'APP_OWNER',
    orig_table => 'ORDERS',
    int_table  => 'ORDERS_NEW'
  );
END;
/

-- (4) Copy dependent objects (constraints, indexes, triggers, grants)
DECLARE
  v_num_errors PLS_INTEGER;
BEGIN
  DBMS_REDEFINITION.COPY_TABLE_DEPENDENTS(
    uname            => 'APP_OWNER',
    orig_table       => 'ORDERS',
    int_table        => 'ORDERS_NEW',
    copy_indexes     => DBMS_REDEFINITION.CONS_ORIG_PARAMS,
    copy_triggers    => TRUE,
    copy_constraints => TRUE,
    copy_privileges  => TRUE,
    ignore_errors    => FALSE,
    num_errors       => v_num_errors
  );
  IF v_num_errors > 0 THEN
    RAISE_APPLICATION_ERROR(-20001, 'COPY_TABLE_DEPENDENTS had errors: ' || v_num_errors);
  END IF;
END;
/

-- (5) Final synchronization and atomic swap: ORDERS_NEW becomes ORDERS
BEGIN
  DBMS_REDEFINITION.FINISH_REDEF_TABLE(
    uname        => 'APP_OWNER',
    orig_table   => 'ORDERS',
    int_table    => 'ORDERS_NEW'
  );
END;
/

-- (6) Cleanup: after verifying the new table, drop the old interim
DROP TABLE ORDERS_NEW PURGE;
```

**Abort — human confirmation required** before discarding an in-flight redefinition:

```sql
BEGIN
  DBMS_REDEFINITION.ABORT_REDEF_TABLE(
    uname      => 'APP_OWNER',
    orig_table => 'ORDERS',
    int_table  => 'ORDERS_NEW'
  );
END;
/

DROP TABLE ORDERS_NEW PURGE;
```

### Monitoring Progress

*No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT SID, SERIAL#, OPNAME, SOFAR, TOTALWORK,
       ROUND(SOFAR / NULLIF(TOTALWORK, 0) * 100, 1) AS PCT_COMPLETE,
       TIME_REMAINING AS SECS_REMAINING
FROM   V$SESSION_LONGOPS
WHERE  OPNAME LIKE '%redefinition%' OR OPNAME LIKE '%Table Redefinition%';
```

While it runs: watch impact with `oracle_long_running_sql` and `oracle_list_active_sessions`; if the copy stalls, drill in with `oracle_session_wait_details`. Around cutover, `oracle_blocking_sessions` confirms the brief exclusive lock is not queueing application sessions. After the swap, run `oracle_list_invalid_objects` to catch objects invalidated by the redefinition.

---

## Online Index Rebuild

Indexes accumulate inefficiency over time: deleted entries leave empty leaf blocks, monotonically increasing keys create right-side imbalance, and data movement degrades clustering factor.

**Preconditions / risks / human confirmation required:** plain `REBUILD` blocks all DML on the table for its duration — production use needs an approved window; prefer `REBUILD ONLINE` (longer, more I/O, ~1x index size of extra space — verify with `oracle_tablespace_usage`).

```sql
-- Offline rebuild: table DML is blocked during rebuild
ALTER INDEX IDX_ORDERS_CUSTOMER REBUILD;

-- Online rebuild: DML continues via a journal table; longer, more I/O,
-- but does not block the application
ALTER INDEX IDX_ORDERS_CUSTOMER REBUILD ONLINE;

-- Rebuild one partition of a partitioned index
ALTER INDEX IDX_ORDERS_DATE REBUILD PARTITION P_2024_Q1 ONLINE;

-- Coalesce: cheaper than rebuild (merges leaf blocks, no sorting);
-- does not reduce height or move to a new tablespace
ALTER INDEX IDX_ORDERS_CUSTOMER COALESCE;
```

Storage options (`TABLESPACE`, `PARALLEL`, `COMPUTE STATISTICS`) can be added to either rebuild form; reset `NOPARALLEL` afterwards.

### Deciding Whether to Rebuild

Call `oracle_table_indexes` on the table — check the index exists, its status (`UNUSABLE` needs attention regardless of structure), and its size. Then run `oracle_stale_stats`: stale optimizer stats distort the row counts used below.

*No dedicated tool — run via `oracle_execute_sql`:*

```sql
ANALYZE INDEX IDX_ORDERS_CUSTOMER VALIDATE STRUCTURE;

SELECT NAME, HEIGHT, BLOCKS, LF_ROWS, LF_BLKS, DEL_LF_ROWS,
       ROUND(DEL_LF_ROWS / NULLIF(LF_ROWS, 0) * 100, 2) AS PCT_DELETED
FROM   INDEX_STATS;
-- If PCT_DELETED > 20% or HEIGHT > 4 for a B-tree, consider rebuilding
```

---

## Online Index Creation

The standard approach for adding new indexes to large production tables — DML continues during the build. The same `ONLINE` pattern applies to `CREATE UNIQUE INDEX` and function-based indexes.

**Preconditions / risks:** needs temp/undo headroom; `NOLOGGING` cuts redo sharply but requires a full backup immediately after creation.

```sql
-- Standard online index creation
CREATE INDEX IDX_ORDERS_STATUS
    ON ORDERS (STATUS_CODE, ORDER_DATE)
    ONLINE
    TABLESPACE IDX_TS
    PARALLEL 4
    COMPUTE STATISTICS
    NOLOGGING;   -- Reduces redo generation; requires full backup after creation

-- After creation, reset to logging for ongoing maintenance
ALTER INDEX IDX_ORDERS_STATUS LOGGING;
```

**Limitations:** bitmap indexes cannot be created or rebuilt online (`ORA-08106`) and are unsuitable for concurrent-DML OLTP tables anyway; IOT secondary indexes cannot always be rebuilt online; UNUSABLE partitions of partitioned indexes must be rebuilt offline; online operations consume more undo and temp space than offline equivalents.

---

## ALTER TABLE ... ONLINE

Oracle 12c added `ONLINE` support for several operations, reducing lock contention. Adding a NOT NULL column with DEFAULT is metadata-only and instantaneous (12c+); widening a VARCHAR2 is always safe.

**Preconditions / risks / human confirmation required:** other modifications can still lock large tables — confirm scope before running anything beyond additive changes.

```sql
-- 12c+: NOT NULL with DEFAULT is metadata-only — instantaneous; existing rows
-- get the default at read time without a physical update
ALTER TABLE ORDERS
ADD (LAST_MODIFIED TIMESTAMP DEFAULT SYSTIMESTAMP NOT NULL);

-- Explicitly mark the operation as online (no-op in 12c+ for this case)
ALTER TABLE ORDERS ADD (AUDIT_USER VARCHAR2(100)) ONLINE;

-- Increase VARCHAR2 width (always safe, no data conversion)
ALTER TABLE CUSTOMERS MODIFY (EMAIL VARCHAR2(320)) ONLINE;

-- Add a DEFAULT to an existing nullable column
ALTER TABLE ORDERS MODIFY (NOTES VARCHAR2(4000) DEFAULT 'N/A') ONLINE;
```

### Setting Columns Unused

Rather than dropping a column immediately (which rewrites all rows), mark it unused first — invisible to the application instantly; space is reclaimed later in a maintenance window.

```sql
ALTER TABLE ORDERS SET UNUSED COLUMN LEGACY_REF_NUM;

ALTER TABLE ORDERS DROP UNUSED COLUMNS;
```

*No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT TABLE_NAME, COUNT(*) AS UNUSED_COLS
FROM   USER_UNUSED_COL_TABS
GROUP  BY TABLE_NAME;
```

### Online Constraint Operations

```sql
-- Enable a disabled constraint without validating existing rows (fast, no lock)
ALTER TABLE ORDERS ENABLE NOVALIDATE CONSTRAINT FK_ORDERS_CUSTOMER;

-- After verifying data quality, enable with full validation
-- (does not prevent DML during the validation scan)
ALTER TABLE ORDERS ENABLE VALIDATE CONSTRAINT FK_ORDERS_CUSTOMER;

-- Add a check constraint that is not validated (future rows only)
ALTER TABLE ORDERS
ADD CONSTRAINT CHK_STATUS CHECK (STATUS_CODE IN ('PENDING','PROCESSING','SHIPPED','CLOSED'))
ENABLE NOVALIDATE;
```

---

## Online Segment Shrink

Shrink reclaims space within a table segment caused by row deletions, without moving the table or requiring downtime.

**Preconditions / risks:** requires ROW MOVEMENT (ROWIDs change — disable it afterwards); the HWM step takes a brief exclusive lock; `CASCADE` also shrinks dependent indexes.

```sql
-- Step 1: enable row movement (rows may change ROWID during shrink)
ALTER TABLE ORDERS ENABLE ROW MOVEMENT;

-- Step 2: compact rows (frees space inside blocks; no HWM change yet; DML not blocked)
ALTER TABLE ORDERS SHRINK SPACE COMPACT;

-- Step 3: adjust High Water Mark (brief exclusive lock, very fast)
ALTER TABLE ORDERS SHRINK SPACE;
-- Alternative: SHRINK SPACE CASCADE does compact + HWM and shrinks dependent indexes

-- After shrinking, disable row movement to re-lock ROWIDs
ALTER TABLE ORDERS DISABLE ROW MOVEMENT;
```

Before deciding to shrink, confirm stats are fresh with `oracle_stale_stats` (the numbers below come from optimizer stats), then measure fragmentation.

*No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT SEGMENT_NAME, BLOCKS, EMPTY_BLOCKS, NUM_ROWS, AVG_ROW_LEN,
       ROUND((BLOCKS * 8192) / 1024 / 1024, 2)          AS TOTAL_MB,
       ROUND((NUM_ROWS * AVG_ROW_LEN) / 1024 / 1024, 2) AS DATA_MB
FROM   USER_TABLES
WHERE  SEGMENT_NAME = 'ORDERS';
```

---

## Minimizing Downtime: Strategy Summary

### Additive Changes (Zero Downtime)

Safe to apply while the application is running: adding nullable columns; adding NOT NULL columns with DEFAULT (12c+); creating new indexes (`CREATE INDEX ... ONLINE`), tables, sequences, synonyms; adding constraints with `ENABLE NOVALIDATE`; creating or replacing views, packages, procedures (if semantically compatible).

### Destructive or Risky Changes (Require Planning)

| Change | Risk | Mitigation |
|---|---|---|
| Dropping a column | Application may still reference it | Set UNUSED first; drop in next release |
| Renaming a column | Immediate application breakage | Add new column, migrate data, switch app, drop old |
| Changing column type | May block or fail on large tables | Use DBMS_REDEFINITION |
| Dropping/truncating a table | Immediate loss | Rename first; drop after application is updated |
| Converting heap to partitioned | Cannot do with ALTER TABLE | Use DBMS_REDEFINITION |

### The Expand/Contract Pattern

1. **Expand** — add the new structure alongside the old; write to both.
2. **Migrate** — backfill existing data; verify consistency.
3. **Contract** — remove the old structure after all application versions using it are retired.

---

## Best Practices

- **Always run `DBMS_REDEFINITION.CAN_REDEF_TABLE` before attempting redefinition.** A failure mid-operation on a large table leaves more mess than never starting.
- **Monitor long operations throughout** — these can run for hours on large tables; visibility into progress is critical.
- **Plan temporary space.** Online index builds need ~1x the index size; full redefinition ~1x the table size.
- **Rebuild with `PARALLEL` during low-traffic windows** and reset `NOPARALLEL` afterwards — high-parallelism rebuilds consume significant I/O bandwidth even though DML is not blocked.
- **Use `NOLOGGING` for initial index builds, then `LOGGING` for maintenance** — and take a backup immediately after any NOLOGGING build.
- **Test online operations in non-production first.** Edge cases (LOB columns, domain indexes, spatial indexes) can prevent online operations from succeeding.

---

## Common Mistakes

**Mistake: Assuming online means instantaneous.** Online operations do not block DML but still consume hours, I/O, and temp space on large objects. Plan and monitor.

**Mistake: Forgetting to synchronize before FINISH_REDEF_TABLE.** Run `SYNC_INTERIM_TABLE` several times in the hours before cutover — it shortens the exclusive-lock window during the swap.

**Mistake: Using REBUILD ONLINE on bitmap indexes.** Oracle raises `ORA-08106`; bitmap indexes must be rebuilt offline in a maintenance window.

**Mistake: Not disabling ROW MOVEMENT after a shrink.** ROWIDs become unstable; anything caching ROWIDs will retrieve wrong rows.

**Mistake: Relying on SET UNUSED for data security.** The column data still exists in the data files. For sensitive data (PII, credentials), nullify values with DELETE/UPDATE before marking unused.

---

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance in this file is valid for Oracle Database 19c unless a newer minimum version is explicitly called out.
- Features marked as 21c, 23c, or 23ai should be treated as Oracle Database 26ai-capable features; keep 19c-compatible alternatives for mixed-version estates.
- For dual-support environments, test syntax and package behavior in both 19c and 26ai because defaults and deprecations can differ by release update.

## Sources

- [DBMS_REDEFINITION (19c)](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_REDEFINITION.html) — START_REDEF_TABLE, FINISH_REDEF_TABLE, ABORT_REDEF_TABLE, SYNC_INTERIM_TABLE, COPY_TABLE_DEPENDENTS, CAN_REDEF_TABLE parameters and constants
- [Oracle Database Administrator's Guide 19c — Online Operations](https://docs.oracle.com/en/database/oracle/oracle-database/19/admin/managing-tables.html) — ALTER TABLE ONLINE, shrink, unused columns
- [Oracle Database SQL Language Reference 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/) — CREATE INDEX ONLINE, REBUILD ONLINE, ALTER TABLE SET UNUSED
