# Oracle Real Application Clusters (RAC) Architecture

## Overview

Oracle RAC is a shared-disk cluster technology allowing multiple Oracle instances to mount and open the same database simultaneously. Each node runs its own instance (memory + background processes) while all nodes share the same datafiles on shared storage (ASM or a cluster file system). RAC provides high availability through instance survivability and horizontal scalability, and is commonly combined with Data Guard for HA within a site and DR across sites.

---

## 1. Core Architecture Components

### Shared Storage
All nodes access the same datafiles, control files, redo log groups, and archived logs. ASM is the recommended storage layer (striping, mirroring, ACFS for non-database files). Each node runs its own ASM instance (`+ASM1`, `+ASM2`, ...).

### Oracle Clusterware (Grid Infrastructure)
Installed before RAC databases. Key components:

| Component | Purpose |
|---|---|
| OCR (Oracle Cluster Registry) | Cluster topology, resource definitions, configuration |
| Voting Disk | Node eviction decisions during split-brain |
| CSS | Heartbeat and membership management |
| CRS | Resource management — DB, VIPs, SCAN |
| CTSS | Time synchronization when NTP is not used |
| SCAN | Single hostname resolving to 1–3 IPs for client load balancing (recommend 3) |

### RAC-Specific Background Processes

| Process | Description |
|---|---|
| LMS | Serves remote block-transfer requests (multiple per instance) |
| LMD | Manages enqueue requests across instances |
| LCK | Instance locks (non-PCM) |
| LMON | Global enqueue recovery; monitors reconfiguration |
| DIAG | Diagnostics for global resource issues |
| RMSn | RAC resource management |

---

## 2. Cache Fusion and the Interconnect

Cache Fusion moves data blocks between instances over the private interconnect instead of disk. Example flow: Instance 1 caches block 42 → Instance 2 requests it → GCS instructs Instance 1 to ship the block over the interconnect → Instance 2 receives it without disk I/O. A **CR transfer** ships an older image for read consistency; a **current block transfer** ships the latest version for modification.

The interconnect must be a **private**, low-latency (< 1 ms RTT), high-bandwidth network (10 GbE minimum; 25 GbE/InfiniBand for heavy loads) with bonded NICs — never routed over public interfaces.

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
-- Interconnect configuration (is_public must be 'NO')
SELECT inst_id, name, ip_address, is_public
FROM gv$cluster_interconnects ORDER BY inst_id, name;

-- Cross-instance block transfer volumes
SELECT inst_id, name, value FROM gv$sysstat
WHERE name IN ('gc cr blocks received', 'gc current blocks received',
               'gc cr blocks served', 'gc current blocks served')
ORDER BY inst_id;
```

---

## 3. Global Cache Service (GCS) and Global Enqueue Service (GES)

**GCS** tracks the state of every data block across instances via a master instance and **PCM locks**. Block states: `LOCAL`, `SHARED`, `NULL`.

**GES** manages non-cache-fusion global resources: dictionary/DML/DDL locks, sequences — ensuring conflicting locks across instances are impossible.

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
-- Cache Fusion efficiency: % of reads served by block transfers instead of disk
SELECT inst_id,
       ROUND((gc_received) / NULLIF(physical_reads + gc_received, 0) * 100, 2) AS cache_fusion_pct
FROM (SELECT inst_id,
             SUM(CASE WHEN name IN ('gc cr blocks received','gc current blocks received')
                      THEN value ELSE 0 END) AS gc_received,
             SUM(CASE WHEN name = 'physical reads' THEN value ELSE 0 END) AS physical_reads
      FROM gv$sysstat
      WHERE name IN ('gc cr blocks received','gc current blocks received','physical reads')
      GROUP BY inst_id) ORDER BY inst_id;

-- Hot segments causing cross-instance transfers (gc buffer busy)
SELECT inst_id, owner, object_name,
       SUM(CASE WHEN statistic_name = 'gc buffer busy waits' THEN value ELSE 0 END) gc_buffer_busy
FROM gv$segment_statistics
WHERE statistic_name = 'gc buffer busy waits'
GROUP BY inst_id, owner, object_name
HAVING SUM(value) > 0
ORDER BY gc_buffer_busy DESC FETCH FIRST 20 ROWS ONLY;
```

Cross-instance contention on a single table can also be localized with `oracle_top_segments` / `oracle_segment_type_summary` before digging into the gc statistics above.

---

## 4. RAC Services

Services direct workload to specific nodes and enable transparent failover; clients connect to a service name, never directly to an instance.

| Type | Description |
|---|---|
| Preferred/Available | Runs on preferred nodes; fails over to available nodes |
| Uniform | Runs on all nodes |
| Administrator-managed | Preferred/available instances defined manually |
| Policy-managed | GI manages instance counts via server pools |

**Preconditions:** create/change services with `srvctl` (or `DBMS_SERVICE`) in a change window.
**Risk:** stopping a service disconnects its sessions.

```bash
# Preferred: manage services with SRVCTL on a cluster node
srvctl add service -db MYDB -service OLTP_SVC -preferred MYDB1,MYDB2 -available MYDB3
srvctl config service -db MYDB -service OLTP_SVC
srvctl start service -db MYDB -service OLTP_SVC
```

```sql
-- Programmatic alternative (no SRVCTL-managed equivalent of all attributes)
BEGIN
    DBMS_SERVICE.CREATE_SERVICE(
        service_name => 'OLTP_SVC',
        network_name => 'OLTP_SVC',
        goal         => DBMS_SERVICE.GOAL_THROUGHPUT,
        clb_goal     => DBMS_SERVICE.CLB_GOAL_LONG);
END;
/
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT inst_id, name, goal FROM gv$active_services
WHERE name NOT IN ('SYS$BACKGROUND','SYS$USERS') ORDER BY name, inst_id;
```

---

## 5. Node Affinity

Binding workloads to nodes reduces Cache Fusion traffic: data touched only by one node never crosses the interconnect. The reliable form is service-based routing — OLTP → service on nodes 1-2, reporting → service on nodes 3-4, batch → a dedicated node — so each dataset lives predominantly in that node group's buffer cache. With policy-managed databases and server pools, **partition affinity** extends this: partition a table by region/tenant and map each partition's access to a node set via region-specific services.

---

## 6. RAC-Specific Wait Events

| Wait Event | Cause | Threshold |
|---|---|---|
| `gc buffer busy acquire` | Waiting for a block being shipped from remote | < 1ms avg |
| `gc buffer busy release` | Another local session holds a block requested remotely | < 1ms avg |
| `gc cr/current block busy` | Remote instance has not yet shipped the block | < 2ms avg |
| `gc cr/current block 2-way` | Normal 2-way transfer | < 1ms |
| `gc cr/current block 3-way` | Requester + master + holder; higher latency | < 2ms |
| `gcs log flush sync` | Remote redo flush before block transfer | Check redo |
| `enq: TX - row lock contention` | Row lock held on another instance | Application issue |

Diagnose with tools instead of raw views: call `oracle_wait_events` → look for `gc %` / `gcs %` events and their average wait; then `oracle_session_wait_details` for affected sessions, and `oracle_ash_top_events` / `oracle_ash_aas_trend` to see when the contention window occurred.

---

## 7. TAF, FCF, and Application Continuity

### Transparent Application Failover (TAF)
Client-side failover in the TNS descriptor: on instance failure the client reconnects to a survivor and can replay the current SELECT. Limitations: in-flight DML is **not** replayed; `TYPE=SELECT` re-executes from the beginning; no protection mid-transaction against network partitions.

```
MYDB_TAF =
  (DESCRIPTION =
    (FAILOVER = ON)
    (LOAD_BALANCE = OFF)
    (ADDRESS = (PROTOCOL = TCP)(HOST = node1-vip)(PORT = 1521))
    (ADDRESS = (PROTOCOL = TCP)(HOST = node2-vip)(PORT = 1521))
    (CONNECT_DATA =
      (SERVICE_NAME = OLTP_SVC)
      (FAILOVER_MODE =
        (TYPE = SELECT)    -- or SESSION
        (METHOD = BASIC)   -- or PRECONNECT
        (RETRIES = 30)
        (DELAY = 5))))
```

### Fast Connection Failover (FCF)
ONS-driven pool invalidation for JDBC/UCP: stale connections are removed proactively when a service goes down; combines with Application Continuity.

### Application Continuity (AC) / Transparent AC (TAC)
AC (12.1+) replays in-flight requests including DML after recoverable errors; TAC (18c+) needs no application changes (`failover_type => 'AUTO'`).

**Risk:** enabling `commit_outcome`/replay changes commit semantics visible to the app — test with the application team.

```sql
BEGIN
    DBMS_SERVICE.MODIFY_SERVICE(
        service_name      => 'OLTP_SVC',
        failover_type     => 'TRANSACTION',  -- 'AUTO' for TAC (18c+)
        commit_outcome    => TRUE,
        retention_timeout => 86400);
END;
/
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT name, failover_type, failover_method, commit_outcome
FROM dba_services WHERE name = 'OLTP_SVC';
```

---

## 8. Cluster Verification Utility (CVU)

`cluvfy` validates network, OS parameters, shared storage, and cluster health:

```bash
cluvfy stage -pre crsinst -n node1,node2 -verbose   # before GI install
cluvfy stage -post crsinst -n node1,node2           # after install
cluvfy comp ocr -n node1,node2                      # OCR integrity
cluvfy comp ssa -n node1,node2 -s <diskgroup>       # shared storage
# Voting disk / OCR from OS (root): crsctl query css votedisk ; ocrcheck
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
-- All instances open?
SELECT inst_id, instance_name, host_name, status, database_status
FROM gv$instance ORDER BY inst_id;

-- Datafiles consistent across instances?
SELECT inst_id, file#, status FROM gv$datafile_header
WHERE status != 'ONLINE' ORDER BY inst_id, file#;
```

---

## 9. Best Practices

- Use SCAN for all client connections; never hardcode VIPs.
- Keep Grid Infrastructure and Database homes separate (independent patching).
- Size the interconnect for peak block-transfer load; a saturated interconnect is the leading cause of RAC degradation.
- Map services 1:1 to workload types (OLTP, reporting, batch) — never mix workloads in one service.
- Never run `ALTER SYSTEM` in RAC without `SID='*'` or `SID='<sid>'`; a per-instance mistake can crash that instance.
- Prefer policy-managed databases for new deployments.
- Enable Cluster Health Monitor (CHM) for 1-second OS metrics during eviction post-mortems.

---

## 10. Common Mistakes

**Interconnect on the public network** — missing private host entries make Oracle default to public NICs; massive GCS latency. Verify with the `gv$cluster_interconnects` query above (`is_public = 'NO'`).

**One UNDO tablespace for all instances** — RAC requires one per instance. *No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT inst_id, value AS undo_tablespace FROM gv$parameter
WHERE name = 'undo_tablespace' ORDER BY inst_id;
-- Each inst_id needs a unique undo tablespace (UNDOTBS1, UNDOTBS2, ...)
```

**Right-hand index contention on sequence keys** — all instances race on the same hot leaf block (`gc buffer busy`). Fix with reverse key indexes or large sequence caches:
```sql
CREATE INDEX ix_orders_order_id ON orders (order_id) REVERSE;

CREATE SEQUENCE seq_orders START WITH 1 INCREMENT BY 1
  CACHE 1000 NOORDER;  -- 1000 values per instance; less GES traffic
```

**Ignoring node eviction root causes** — evictions usually stem from frozen processes, slow interconnect heartbeats, or missed `misscount` timeouts, not hardware. Analyze `cssd.log`, the DB alert log, and CHM data together.

**Patching without `opatchauto`** — manual rolling patches can leave GI/DB homes inconsistent across nodes; use `opatchauto` for GI and RAC.

---

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance valid for Oracle Database 19c unless a newer minimum is explicitly stated.
- Features marked 21c/23c/23ai should be treated as 26ai-capable; keep 19c-compatible alternatives for mixed-version estates.
- In dual-support environments, test syntax and behavior on both 19c and 26ai; defaults and deprecations differ by release update.

## Sources

- [Oracle RAC Administration and Deployment Guide 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/racad/) — architecture, GCS/GES, services, TAF, CVU
- [Oracle RAC 19c Glossary](https://docs.oracle.com/en/database/oracle/oracle-database/19/racad/glossary.html) — LMON, LMSn, LMD, Cache Fusion, SCAN, OCR, CSS definitions
- [About RAC Background Processes](https://docs.oracle.com/database/121/RACAD/GUID-AEBD3F49-4F10-4BDE-9008-DC1AF8E7DB42.htm) — LMS, LMD, LCK, LMON, DIAG, RMSn
- [Connecting to Oracle RAC Using SCANs](https://docs.oracle.com/en/database/oracle/oracle-database/19/rilin/about-connecting-to-an-oracle-rac-database-using-scans.html)
- [Ensuring Application Continuity (19c)](https://docs.oracle.com/en/database/oracle/oracle-database/19/racad/ensuring-application-continuity.html)
- [DBMS_SERVICE (19c)](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_SERVICE.html)
