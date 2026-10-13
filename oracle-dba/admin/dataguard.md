# Oracle Data Guard — Operations Runbook

## Overview

Oracle Data Guard maintains one or more synchronized copies of a production database (the **primary**) called **standby databases**. If the primary becomes unavailable, a standby is activated to take over, minimizing downtime and data loss. It is licensed with Oracle Database Enterprise Edition and is the recommended DR solution for mission-critical databases (core MAA component).

This file focuses on operational procedures. For standby types, protection-mode theory, redo transport/apply architecture, initialization parameters, and the monitoring-view reference, see `architecture/dataguard.md`.

---

## Routine Health Checks (tool-driven)

Run on every inspection and around any role transition:

- **Role/readiness** — `oracle_dg_role_snapshot` → `database_role`, `protection_mode`, `switchover_status` (`TO STANDBY`/`SESSIONS ACTIVE` = ready), `fs_failover_status`.
- **Transport** — `oracle_dg_transport_status` → compare `archived_seq#` vs `applied_seq#` per standby destination; then `oracle_archive_dest_status` → `error` empty, `status` = `VALID`.
- **Gaps** — `oracle_dg_archive_gap` (on the standby) → any row is a missing log range; resolve before any role transition.
- **Lag** — `oracle_dataguard_stats` (standby) → `transport lag` / `apply lag` vs your RTO/RPO budget.
- **Apply processes** — `oracle_dg_processes` (19c; `V$MANAGED_STANDBY`) → `MRP0` = `APPLYING_LOG` or `WAIT_FOR_LOG`.
- **Standby redo logs** — `oracle_dg_standby_logs` → (primary groups + 1) per thread, same size as online logs, none stuck in `CLEARING`.

---

## Snapshot Standby Conversion

**Preconditions:** Flashback Database enabled on the standby; database is currently a physical standby (confirm via `oracle_dg_role_snapshot`).
**Risk:** all local writes are discarded when converting back — get written approval if test data must survive. **Requires explicit confirmation.**

```bash
DGMGRL> CONVERT DATABASE standby_db TO SNAPSHOT STANDBY;
# Later, to resume protection:
DGMGRL> CONVERT DATABASE standby_db TO PHYSICAL STANDBY;
```

Re-run `oracle_dg_role_snapshot` afterwards to confirm `database_role` flipped as expected.

---

## Redo Transport Configuration

**Preconditions:** unique `DB_UNIQUE_NAME` per member; `LOG_ARCHIVE_CONFIG=DG_CONFIG(...)` set on every member.
**Risk:** a wrong `VALID_FOR` stops transport silently — verify with `oracle_dg_transport_status` immediately after the change.

```sql
-- Configure redo transport on primary (example LOG_ARCHIVE_DEST_2)
ALTER SYSTEM SET LOG_ARCHIVE_DEST_2 =
  'SERVICE=standby_tns ASYNC NOAFFIRM
   DB_UNIQUE_NAME=standby_db
   VALID_FOR=(ONLINE_LOGFILES,PRIMARY_ROLE)'
  SCOPE=BOTH;
ALTER SYSTEM SET LOG_ARCHIVE_DEST_STATE_2 = ENABLE SCOPE=BOTH;
```

---

## Apply Control

**Physical standby (managed recovery):**
```sql
-- Start real-time apply (applies redo as it arrives)
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE
  USING CURRENT LOGFILE DISCONNECT FROM SESSION;

-- Stop managed recovery
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE CANCEL;
```

`USING CURRENT LOGFILE` is deprecated since 12.1 — real-time apply starts automatically when standby redo logs exist. Verify with `oracle_dg_processes` → `MRP0` = `APPLYING_LOG`.

**Logical standby (SQL Apply):**
```sql
ALTER DATABASE START LOGICAL STANDBY APPLY IMMEDIATE;
ALTER DATABASE STOP LOGICAL STANDBY APPLY;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT status, applied_scn, latest_scn FROM dba_logstdby_progress;
```

---

## Standby Redo Logs

**Sizing rule:** (number of primary online redo groups + 1) per thread; each group the same size as the primary online redo logs. Required for real-time apply and SYNC transport.

```sql
ALTER DATABASE ADD STANDBY LOGFILE GROUP 4
  ('/oradata/standby/stdby_redo04.log') SIZE 500M;
```

Verify layout and status with `oracle_dg_standby_logs`.

---

## Data Guard Broker

Strongly recommended over manual `LOG_ARCHIVE_DEST_n` management.

**Preconditions:** password file authentication in place; connectivity between members. **Risk:** enabling/creating a configuration changes how redo transport is managed — do outside peak if possible.

```sql
-- Enable on both primary and standby
ALTER SYSTEM SET dg_broker_start = TRUE SCOPE=BOTH;
```

```bash
dgmgrl sys/<password>@primary_db

DGMGRL> CREATE CONFIGURATION 'my_dg_config'
          AS PRIMARY DATABASE IS primary_db
          CONNECT IDENTIFIER IS primary_tns;
DGMGRL> ADD DATABASE standby_db
          AS CONNECT IDENTIFIER IS standby_tns
          MAINTAINED AS PHYSICAL;
DGMGRL> ENABLE CONFIGURATION;

# Health
DGMGRL> SHOW CONFIGURATION;
DGMGRL> SHOW DATABASE VERBOSE standby_db;
DGMGRL> VALIDATE DATABASE standby_db;
```

---

## Switchover (Planned Role Reversal)

**Preconditions:** `switchover_status` = `TO STANDBY`/`SESSIONS ACTIVE` on both sides (`oracle_dg_role_snapshot`); near-zero lag (`oracle_dataguard_stats`); no long-running SQL (`oracle_long_running_sql`) or blocking sessions (`oracle_blocking_sessions`).
**Risk:** brief outage of both databases; reversible. **Requires a change window and explicit confirmation.**

```bash
DGMGRL> VALIDATE DATABASE standby_db;
DGMGRL> SWITCHOVER TO standby_db;
DGMGRL> SHOW CONFIGURATION;
```

Manual (without Broker):
```sql
-- On PRIMARY: initiate switchover
ALTER DATABASE COMMIT TO SWITCHOVER TO PHYSICAL STANDBY WITH SESSION SHUTDOWN;

-- On STANDBY: complete and open as primary
ALTER DATABASE COMMIT TO SWITCHOVER TO PRIMARY WITH SESSION SHUTDOWN;
ALTER DATABASE OPEN;
```

---

## Failover (Emergency)

**Preconditions:** primary unavailable and not quickly recoverable; check gaps first with `oracle_dg_archive_gap` (on the standby).
**Risk:** irreversible — the old primary cannot serve until reinstated; data loss possible unless Maximum Availability/Protection with all redo received. `IMMEDIATE` skips applying remaining redo and increases data loss.
**Requires incident approval and explicit human confirmation.**

```bash
DGMGRL> FAILOVER TO standby_db;
# Last resort only:
DGMGRL> FAILOVER TO standby_db IMMEDIATE;
```

Manual (without Broker):
```sql
RECOVER MANAGED STANDBY DATABASE CANCEL;
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE FINISH;
ALTER DATABASE ACTIVATE PHYSICAL STANDBY DATABASE;
ALTER DATABASE OPEN;
```

### Reinstate the Old Primary

Requires Flashback Database enabled **before** the failover:
```bash
DGMGRL> REINSTATE DATABASE old_primary_db;
```

---

## Lag Monitoring

Primary method: `oracle_dataguard_stats` (standby) for `transport lag` / `apply lag`; `oracle_dg_transport_status` (primary) for per-destination archived-vs-applied sequences; `oracle_dg_archive_gap` (standby) for gaps. DGMGRL cross-check:

```bash
DGMGRL> SHOW DATABASE standby_db 'ApplyLag';
DGMGRL> SHOW DATABASE standby_db 'TransportLag';
DGMGRL> SHOW DATABASE standby_db 'RecvQEntries';   -- redo awaiting transport
DGMGRL> SHOW DATABASE standby_db 'SendQEntries';   -- redo awaiting apply
```

---

## Protection Mode Change

**Preconditions:** for PROTECTION/AVAILABILITY, at least one standby with SYNC transport and standby redo logs configured (`oracle_dg_standby_logs`, `oracle_dg_transport_status`).
**Risk:** MAXIMUM PROTECTION shuts the primary down if no synchronized standby is reachable. **Requires change approval.**

```sql
ALTER DATABASE SET STANDBY DATABASE TO MAXIMIZE PROTECTION;
ALTER DATABASE SET STANDBY DATABASE TO MAXIMIZE AVAILABILITY;
ALTER DATABASE SET STANDBY DATABASE TO MAXIMIZE PERFORMANCE;
```

Confirm afterwards with `oracle_dg_role_snapshot` → `protection_mode`.

---

## Active Data Guard (Read-Only Open)

Requires a separate ADG license to keep apply running while the standby is open.

```sql
ALTER DATABASE OPEN READ ONLY;
ALTER DATABASE RECOVER MANAGED STANDBY DATABASE
  USING CURRENT LOGFILE DISCONNECT FROM SESSION;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT open_mode FROM v$database;   -- expect: READ ONLY WITH APPLY
```

---

## Best Practices

- Use the Broker for all configuration; manual `LOG_ARCHIVE_DEST_n` management is error-prone.
- Configure standby redo logs on **both** primary and standby, sized as above.
- Use Maximum Availability + SYNC for critical OLTP when RTT to the standby is < ~5 ms.
- Monitor lag actively: 4 hours of apply lag = 4 hours of recovery time, not instant failover.
- Test switchovers quarterly; an untested failover procedure is a high-risk assumption.
- Back up from the standby with RMAN (check coverage with `oracle_rman_backup_summary`).
- Enable Flashback Database on the primary to allow reinstatement after failover:
  ```sql
  ALTER DATABASE FLASHBACK ON;
  ```

---

## Common Mistakes

**No standby redo logs** — real-time apply impossible, apply lag grows. Detect: `oracle_dg_standby_logs` returns no groups.

**Wrong `VALID_FOR`** — redo transport stops silently. Correct pattern:
```sql
ALTER SYSTEM SET LOG_ARCHIVE_DEST_2 =
  'SERVICE=standby ASYNC NOAFFIRM
   DB_UNIQUE_NAME=standby
   VALID_FOR=(ONLINE_LOGFILES,PRIMARY_ROLE)';
```

**Duplicate or missing `DB_UNIQUE_NAME`** — members may share `DB_NAME` but never `DB_UNIQUE_NAME`.
*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT db_unique_name, db_name FROM v$database;
```

**Failover without checking gaps** — always call `oracle_dg_archive_gap` on the standby first.

**No Flashback on the primary** — reinstatement after failover requires a full rebuild from backup.

**Opening a standby read-only without an ADG license** — suspending apply to query the standby is not Active Data Guard.

---

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance valid for Oracle Database 19c unless a newer minimum is explicitly stated.
- Features marked 21c/23c/23ai should be treated as 26ai-capable; keep 19c-compatible alternatives for mixed-version estates.
- In dual-support environments, test syntax and behavior on both 19c and 26ai; defaults and deprecations differ by release update.

## Sources

- [Oracle Data Guard Concepts and Administration 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/sbydb/)
- [Oracle 19c SQL Language Reference — ALTER DATABASE](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/ALTER-DATABASE.html)
- [Oracle Data Guard Broker 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/dgbkr/)
- [Oracle 19c Reference — V$DATAGUARD_STATS](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-DATAGUARD_STATS.html)
