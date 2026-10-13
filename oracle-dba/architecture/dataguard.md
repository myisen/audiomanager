# Oracle Data Guard — Concepts and Architecture

## Overview

Oracle Data Guard maintains synchronized copies of a production database (the **primary**) called **standby databases**. If the primary becomes unavailable, a standby is activated to take over, minimizing downtime and data loss. It is a native feature of Oracle Database **Enterprise Edition**; a primary can ship redo directly to up to 30 standbys, and cascading scales beyond 30. Two core services: **Redo Transport** (ships redo, resolves gaps automatically) and **Apply Services** (applies received redo on the standby).

For operational procedures (runbook-style commands and checks), see `admin/dataguard.md`.

---

## 1. Standby Database Types

### Physical Standby
Block-for-block identical copy, synchronized via **Redo Apply** (media recovery through MRP0).
- Supports all data types without restriction; most efficient apply (bypasses the SQL layer).
- Can be opened read-only while apply runs (Active Data Guard license).
- Primary block corruptions are **not** propagated; supports RMAN backup offload and rolling patches.

### Logical Standby
Maintained via **SQL Apply** (LogMiner-based): redo is mined into SQL and executed on an open read-write database.
- Standby-maintained tables are protected from user DML by guard rules; can carry extra indexes or materialized views.
- Enables rolling upgrades via `DBMS_ROLLING`; has datatype restrictions (from 12.2, newer features may require `DBMS_ROLLING` or GoldenGate).

### Snapshot Standby
A physical standby temporarily converted to read-write for testing. Redo is received and archived but **not** applied; local writes are discarded on convert-back (Flashback Database internally).

**Preconditions:** Flashback Database enabled. **Risk:** test data destroyed on convert-back.

```sql
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE CANCEL;
ALTER DATABASE CONVERT TO SNAPSHOT STANDBY;
ALTER DATABASE OPEN READ WRITE;
-- Convert back to physical standby
ALTER DATABASE CONVERT TO PHYSICAL STANDBY;
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE DISCONNECT FROM SESSION;
```

Via Broker:
```bash
DGMGRL> CONVERT DATABASE 'standby_db' TO SNAPSHOT STANDBY;
DGMGRL> CONVERT DATABASE 'standby_db' TO PHYSICAL STANDBY;
```

### Far Sync Instance
Lightweight remote instance (control file only — no datafiles) that receives redo from the primary and forwards it to terminal standbys — zero data loss at distant sites without long-distance synchronous transmission. Cannot be opened, apply redo, or become primary; requires ADG Far Sync license.

```bash
DGMGRL> ADD FAR_SYNC farsync_inst AS CONNECT IDENTIFIER IS farsync_tns;
DGMGRL> EDIT DATABASE primary_db SET PROPERTY
          RedoRoutes='(LOCAL : farsync_inst SYNC)(farsync_inst : standby_db ASYNC)';
DGMGRL> ENABLE FAR_SYNC farsync_inst;
```

---

## 2. Protection Modes

- **Maximum Performance (default)** — commit as soon as local redo is written; `ASYNC NOAFFIRM`; potential data loss = transport lag.
- **Maximum Availability (recommended for mission-critical)** — commit waits for standby acknowledgment (`SYNC`); falls back to Maximum Performance if all SYNC standbys are unreachable. `SYNC AFFIRM` = redo written to standby disk; `SYNC NOAFFIRM` (FastSync) = received in standby memory, faster but small risk on simultaneous site failures.
- **Maximum Protection** — `SYNC AFFIRM`; the primary **shuts down** if no synchronized standby is reachable. Only settable from Maximum Availability with a synchronized standby in place; use two standbys so one failure cannot down the primary.

**Risk:** mode changes alter commit behavior fleet-wide; PROTECTION can stop the primary. **Requires change approval.**

```sql
ALTER DATABASE SET STANDBY DATABASE TO MAXIMIZE PERFORMANCE;
ALTER DATABASE SET STANDBY DATABASE TO MAXIMIZE AVAILABILITY;
ALTER DATABASE SET STANDBY DATABASE TO MAXIMIZE PROTECTION;
```

Verify afterwards: call `oracle_dg_role_snapshot` → `protection_mode`.

---

## 3. Redo Transport

### LOG_ARCHIVE_DEST_n

Up to 31 destinations configure where redo is archived/shipped.

```sql
-- Typical primary → standby transport (ASYNC)
ALTER SYSTEM SET LOG_ARCHIVE_DEST_2 =
  'SERVICE=standby_db ASYNC NOAFFIRM
   DB_UNIQUE_NAME=standby_db
   VALID_FOR=(ONLINE_LOGFILES,PRIMARY_ROLE) REOPEN=60'
  SCOPE=BOTH;

-- Synchronous transport (Maximum Availability)
ALTER SYSTEM SET LOG_ARCHIVE_DEST_2 =
  'SERVICE=standby_db SYNC AFFIRM NET_TIMEOUT=30
   DB_UNIQUE_NAME=standby_db
   VALID_FOR=(ONLINE_LOGFILES,PRIMARY_ROLE)'
  SCOPE=BOTH;
ALTER SYSTEM SET LOG_ARCHIVE_DEST_STATE_2 = ENABLE SCOPE=BOTH;
```

Key attributes: `SERVICE` (Net alias), `SYNC`/`ASYNC`, `AFFIRM`/`NOAFFIRM`, `NET_TIMEOUT` (LGWR ack wait, SYNC only), `VALID_FOR=(<log_type>,<role>)`, `DB_UNIQUE_NAME`, `REOPEN` (reconnect interval), `COMPRESSION=ENABLE` (Advanced Compression license), `DELAY=<minutes>` (apply delay, default 30), `MANDATORY`, `ALTERNATE=LOG_ARCHIVE_DEST_n`.

### LOG_ARCHIVE_CONFIG

Required when using `DB_UNIQUE_NAME` in destinations; set on every member:

```sql
ALTER SYSTEM SET LOG_ARCHIVE_CONFIG='DG_CONFIG=(primary_db,standby_db)' SCOPE=BOTH;
```

### Standby Redo Logs (SRLs)

Required for real-time apply and SYNC transport: at least (primary online redo groups + 1) per thread, each group at least as large as the largest online redo log.

```sql
ALTER DATABASE ADD STANDBY LOGFILE THREAD 1 SIZE 500M;
```

Verify with `oracle_dg_standby_logs`.

### Transport Monitoring

- Call `oracle_dg_transport_status` → per standby destination: `archived_seq#` vs `applied_seq#`, `gap_status`.
- Call `oracle_archive_dest_status` → destination config, `status`, and `error`.
- Call `oracle_dg_archive_gap` (on the standby) → missing log ranges.

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT * FROM V$REDO_DEST_RESP_HISTOGRAM WHERE DEST_ID = 2;  -- SYNC response times
```

---

## 4. Apply Services

### Redo Apply (Physical Standby)

```sql
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE DISCONNECT;
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE CANCEL;
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE NODELAY;  -- remove apply delay
```

> `USING CURRENT LOGFILE` is deprecated since 12.1 and no longer required — real-time apply is automatic when SRLs exist.

### SQL Apply (Logical Standby)

```sql
ALTER DATABASE START LOGICAL STANDBY APPLY IMMEDIATE;
ALTER DATABASE STOP LOGICAL STANDBY APPLY;
```

### Apply Progress

- Call `oracle_dataguard_stats` (on the standby) → `transport lag`, `apply lag`, `apply finish time`.
- Call `oracle_dg_processes` (19c, `V$MANAGED_STANDBY`) → `MRP0` state; on 12.2+ prefer `V$DATAGUARD_PROCESS`.
- Call `oracle_dg_role_snapshot` → `database_role`, `open_mode`, `switchover_status`.

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT ROLE, THREAD#, SEQUENCE#, ACTION FROM V$DATAGUARD_PROCESS;
```

---

## 5. Key Initialization Parameters

| Parameter | Description |
|---|---|
| `DB_NAME` / `DB_UNIQUE_NAME` | Shared DB_NAME; unique DB_UNIQUE_NAME per member |
| `LOG_ARCHIVE_CONFIG` | `DG_CONFIG=(...)` list of all members |
| `LOG_ARCHIVE_DEST_n` / `_STATE_n` | Transport destinations; ENABLE/DEFER/ALTERNATE |
| `REMOTE_LOGIN_PASSWORDFILE` | `EXCLUSIVE`/`SHARED` — transport authentication |
| `FAL_SERVER` | Net alias of the server that resolves redo gaps |
| `DB_FILE_NAME_CONVERT` / `LOG_FILE_NAME_CONVERT` | Primary → standby path conversion |
| `STANDBY_FILE_MANAGEMENT` | `AUTO` — manage standby datafiles automatically |
| `DG_BROKER_START` | `TRUE` starts the DMON broker process |
| `STANDBY_DB_PRESERVE_STATES` | Preserve sessions/buffers during transition (12.2+) |
| `ENABLED_PDBS_ON_STANDBY` | Replicate a subset of PDBs (CDB only) |
| `COMPATIBLE` | Must match on all members (logical standby may be higher) |

Review non-default values with `oracle_nondefault_parameters`.

---

## 6. Data Guard Broker (DGMGRL)

Broker automates configuration, monitoring, and role transitions; strongly recommended over manual `LOG_ARCHIVE_DEST_n` management.

```sql
-- Enable on every managed instance (primary and standbys)
ALTER SYSTEM SET DG_BROKER_START = TRUE SCOPE=BOTH;
```

```bash
DGMGRL> CONNECT SYS@primary_db;
DGMGRL> CREATE CONFIGURATION 'DRSolution' AS
          PRIMARY DATABASE IS 'primary_db'
          CONNECT IDENTIFIER IS primary_tns;
DGMGRL> ADD DATABASE 'standby_db'
          AS CONNECT IDENTIFIER IS standby_tns
          MAINTAINED AS PHYSICAL;
DGMGRL> ENABLE CONFIGURATION;

# Health, validation, properties
DGMGRL> SHOW CONFIGURATION;
DGMGRL> SHOW DATABASE VERBOSE 'standby_db';
DGMGRL> VALIDATE DATABASE 'standby_db';
DGMGRL> EDIT DATABASE 'standby_db' SET PROPERTY LogXptMode='SYNC';
```

---

## 7. Switchover and Failover

### Switchover
Planned, graceful role reversal — no data loss; used for maintenance, patching, testing.
**Preconditions:** `switchover_status` = `TO STANDBY`/`SESSIONS ACTIVE` on both sides (`oracle_dg_role_snapshot`); near-zero apply lag (`oracle_dataguard_stats`). **Requires a change window.**

```bash
DGMGRL> VALIDATE DATABASE 'standby_db';
DGMGRL> SWITCHOVER TO 'standby_db';
DGMGRL> SHOW CONFIGURATION;
```

Manual (without Broker):
```sql
-- On primary: verify, then initiate
ALTER DATABASE SWITCHOVER TO standby_db VERIFY;
ALTER DATABASE SWITCHOVER TO standby_db;
ALTER DATABASE OPEN;

-- On new standby (former primary)
STARTUP MOUNT;
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE DISCONNECT FROM SESSION;
```

### Failover
Emergency activation when the primary is unavailable. May lose data unless all redo was received; irreversible without reinstatement. **Requires incident approval and explicit human confirmation.**

```bash
DGMGRL> FAILOVER TO 'standby_db';
DGMGRL> FAILOVER TO 'standby_db' IMMEDIATE;  -- skips unapplied redo; more data loss
```

Manual (without Broker) — check gaps first with `oracle_dg_archive_gap`:
```sql
-- If the primary is mountable, flush remaining redo first
ALTER SYSTEM FLUSH REDO TO standby_db;

ALTER DATABASE RECOVER MANAGED STANDBY DATABASE CANCEL;
ALTER DATABASE FAILOVER TO standby_db;
-- Last resort (accepts data loss):
ALTER DATABASE ACTIVATE PHYSICAL STANDBY DATABASE;
ALTER DATABASE OPEN;
```

### Reinstatement
After failover, Flashback Database rolls the old primary back and re-synchronizes it as a standby. Flashback must be enabled **before** the failover:

```bash
DGMGRL> REINSTATE DATABASE 'old_primary_db';
```

```sql
ALTER DATABASE FLASHBACK ON;
```

---

## 8. Fast-Start Failover (FSFO)

An observer process automatically initiates failover when the primary becomes unavailable. Requires Broker.

```bash
DGMGRL> ENABLE FAST_START FAILOVER;
DGMGRL> START OBSERVER;
DGMGRL> VALIDATE FAST_START FAILOVER;
DGMGRL> ENABLE FAST_START FAILOVER OBSERVE ONLY;  -- test mode, no automatic failover
```

Requirements: Flashback Database on both primary and standby; SRLs on both; observer running (else `ORA-16819`); up to 4 observers (26ai).
**Risk:** automatic failover on perceived primary loss — set `FastStartFailoverThreshold` deliberately. Monitor with `oracle_dg_role_snapshot` → `fs_failover_status`.

---

## 9. Active Data Guard (Read Offload)

A physical standby open **read-only** while applying redo; requires a separate ADG license.

```sql
ALTER DATABASE OPEN READ ONLY;
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE DISCONNECT;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT OPEN_MODE, DB_UNIQUE_NAME FROM V$DATABASE;  -- expect: READ ONLY WITH APPLY
```

Session-level lag tolerance:
```sql
ALTER SESSION SET STANDBY_MAX_DATA_DELAY = 2;  -- reject query if lag > 2s
ALTER SESSION SYNC WITH PRIMARY;               -- block until received redo is applied
```

---

## 10. Monitoring Views Reference

| View | Description |
|---|---|
| `V$DATABASE` | Role, protection mode, open mode, switchover status, FSFO status |
| `V$DATAGUARD_PROCESS` | DG background processes (replaces `V$MANAGED_STANDBY`, 12.2+) |
| `V$DATAGUARD_STATS` | Transport lag, apply lag, apply finish time |
| `V$ARCHIVE_DEST` / `_STATUS` | Destination config and runtime status incl. `GAP_STATUS` |
| `V$ARCHIVE_GAP` | Archive gaps on physical standby |
| `V$STANDBY_LOG` | Standby redo log groups |
| `V$DATAGUARD_STATUS` | DG messages written to the alert log |
| `V$REDO_DEST_RESP_HISTOGRAM` | SYNC transport response times |
| `V$STANDBY_EVENT_HISTOGRAM` | Apply lag histogram |
| `V$FS_FAILOVER_OBSERVERS` | FSFO observer information |

Broker-specific (26ai): `V$DG_BROKER_CONFIG`, `V$DG_BROKER_PROPERTY`, `V$DG_BROKER_ROLE_CHANGE`, `V$DG_BROKER_TAG`, `V$FAST_START_FAILOVER_CONFIG`, `V$FS_LAG_HISTOGRAM`.

Most of these are surfaced through the DG tools: `oracle_dg_role_snapshot`, `oracle_dg_processes`, `oracle_dg_transport_status`, `oracle_dg_archive_gap`, `oracle_dg_standby_logs`, `oracle_dataguard_stats`, `oracle_archive_dest_status`.

---

## 11. Best Practices

- Use the Broker for all operations; manual `LOG_ARCHIVE_DEST_n` management is error-prone.
- Configure SRLs on both primary and standby, sized identically to online redo logs.
- Maximum Availability + SYNC for OLTP when RTT to the standby < ~5 ms.
- Enable Flashback Database on the primary before production use (needed for reinstatement).
- Test switchovers quarterly; monitor lag actively — hours of apply lag = hours of recovery time.
- Back up from the standby with RMAN; set `STANDBY_FILE_MANAGEMENT=AUTO`.

---

## 12. Common Mistakes

**No standby redo logs** — real-time apply and SYNC acknowledgment impossible; apply lag grows.

**Wrong `VALID_FOR`** — log type/role must match the sending database's state, or transport silently stops.

**Missing `DB_UNIQUE_NAME`** — members may share `DB_NAME` but never `DB_UNIQUE_NAME`.

**Failing over without checking gaps** — call `oracle_dg_archive_gap` first.

**No Flashback before failover** — reinstatement then requires a full rebuild from backup.

**Deprecated syntax** — `USING CURRENT LOGFILE` (12.1) and `V$MANAGED_STANDBY` (12.2) still work but should not appear in new scripts; use `V$DATAGUARD_PROCESS`.

---

## 13. Oracle Version Notes (19c vs 26ai)

- **Oracle 19c:** full DG functionality (physical/logical/snapshot, Far Sync, all protection modes, Broker, FSFO); `V$MANAGED_STANDBY` still the process view.
- **Oracle 21c:** Data Guard for PDBs (DG PDB) — PDB-level protection and role transitions via Broker only.
- **Oracle 23ai:** `DBMS_ROLLING` in multitenant environments.
- **Oracle 26ai:** `DBMS_DG` PL/SQL API; up to 4 FSFO observers with preferred affinity; new FSFO properties (`FastStartFailoverLagType`, `FastStartFailoverLagGraceTime`); new DGMGRL commands (`EDIT ALL MEMBERS`, tagging, `VALIDATE DATABASE STRICT`, `PREPARE DATABASE FOR DATA GUARD`); automatic tempfile creation on standbys; SQLcl integration; new monitoring views (section 10); DG PDB enhancements.
- `COMPATIBLE` must be identical on all members except logical standbys (may be higher); `USING CURRENT LOGFILE` deprecated 12.1; `V$MANAGED_STANDBY` deprecated 12.2.

---

## 14. Sources

- [Oracle Data Guard Concepts and Administration, Release 26.1](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/index.html)
- [DG 26.1 — Protection Modes](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/oracle-data-guard-protection-modes.html)
- [DG 26.1 — Redo Transport Services](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/oracle-data-guard-redo-transport-services.html)
- [DG 26.1 — Managing Role Transitions](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/managing-oracle-data-guard-role-transitions.html)
- [Oracle Data Guard Broker, Release 26.1](https://docs.oracle.com/en/database/oracle/oracle-database/26/dgbkr/index.html)
- [Oracle Data Guard Concepts and Administration 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/sbydb/index.html)
