# Space Management

Space exhaustion is one of the most common preventable outage causes. This file covers monitoring (tools), the HWM concept, and the reclamation playbooks (SHRINK / MOVE / LOB) — the reclamation statements are operational and need approval.

## Monitoring with Tools

1. `oracle_tablespace_usage` (min_used_pct=0 for full view) → per tablespace: used %, `has_autoextend`, and a CRITICAL(≥95%)/WARNING(≥85%)/WATCH(≥75%) grade. **Never read used_pct alone**: a tablespace at 50% whose files are capped by MAXSIZE is closer to outage than one at 85% with headroom.
2. `oracle_datafile_usage` → which files are capped (`autoextensible=NO` or near `max_mb`), autoextend step size.
3. ASM storage → `oracle_asm_diskgroup_usage`; FRA → `oracle_fra_usage`.
4. Who is consuming it: `oracle_top_segments` (20 largest), `oracle_segment_type_summary` (by type), `oracle_lob_segments` (LOB segments grow independently of their parent tables).
5. Trending / capacity: `oracle_tablespace_growth_history` (weekly max/min used from AWR; size TEMP for peak, not average — month-end sorts).
6. TEMP diagnostics: `oracle_temp_usage` → `oracle_temp_by_session` (find the session behind ORA-01652).

Set monitoring alerts at 80% used — do not wait for 95%: autoextend may be capped and a bulk load can fill the remainder in minutes.

## High Water Mark

The HWM is the highest block ever used. Full scans read to the HWM, so a table emptied by DELETE still scans everything. `oracle_hwm_waste_tables` (min_alloc_mb param) lists tables where allocation far exceeds the data estimate — reclaim candidates. It relies on `num_rows`/`avg_row_len`: run `DBMS_STATS.gather_table_stats` first if stats are stale (`oracle_stale_stats`).

## Reclaiming Space (operational — approval required)

### SHRINK SPACE (online, ASSM tablespaces only)

```sql
-- Step 1: enable row movement (ROWIDs change — breaks apps that cache ROWIDs!)
ALTER TABLE sales ENABLE ROW MOVEMENT;

-- Step 2: compact during business hours (moves rows, does not reset HWM)
ALTER TABLE sales SHRINK SPACE COMPACT;

-- Step 3: reset HWM and release space (brief exclusive lock) — low traffic window
ALTER TABLE sales SHRINK SPACE;

-- Cascade to dependent indexes, or shrink/rebuild them after
ALTER TABLE sales SHRINK SPACE CASCADE;
```

Use SHRINK when: ASSM tablespace, online required, table had significant DELETE/UPDATE churn. Do not use on IOTs or dictionary-managed tablespaces.

### MOVE (offline, more thorough)

```sql
ALTER TABLE sales MOVE;                       -- same tablespace, resets HWM completely
ALTER TABLE sales MOVE TABLESPACE data_ts;    -- relocate (e.g. slow disk → SSD)
ALTER TABLE sales MOVE ONLINE;                -- 12.2+
```

**After MOVE all indexes are UNUSABLE — rebuild them or every DML hits ORA-01502.** Check with `oracle_table_indexes` (owner, table) → status, then rebuild (operational):

```sql
SELECT 'ALTER INDEX ' || owner || '.' || index_name || ' REBUILD ONLINE;' AS rebuild_stmt
FROM   dba_indexes
WHERE  table_owner = '<OWNER>' AND table_name = '<TABLE>' AND status = 'UNUSABLE';
```

| | SHRINK SPACE | MOVE |
|---|---|---|
| Online | Yes | only with ONLINE (12.2+) |
| Indexes | untouched | UNUSABLE, must rebuild |
| ASSM required | Yes | No |
| Row movement | required | not required |
| Reclamation | very good | complete |

Run the Segment Advisor first for prioritization — see [health-monitor.md](health-monitor.md).

## LOB Space

LOBs have their own HWM and never shrink via table SHRINK:

```sql
-- SecureFiles: online shrink (operational)
ALTER TABLE documents MODIFY LOB (document_content) (SHRINK SPACE);

-- BasicFiles or full reclamation: MOVE to a new segment (operational)
ALTER TABLE documents MOVE LOB (document_content) STORE AS (TABLESPACE lob_ts);
```

Inventory with `oracle_lob_segments`. Excessive BasicFiles RETENTION also inflates LOB segments — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT owner, table_name, column_name, securefile, retention_type, retention_value
FROM   dba_lobs
WHERE  owner NOT IN ('SYS','SYSTEM')
ORDER  BY owner, table_name;
```

## Stuck Temp Space

If TEMP does not release after sessions disconnect (orphaned segments — RAC/bugs):

*No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT t.tablespace, t.blocks, t.sql_id
FROM   v$sort_usage t
WHERE  NOT EXISTS (SELECT 1 FROM v$session s WHERE s.saddr = t.session_addr);
```

```sql
-- 12c+ (operational): shrink TEMP
ALTER TABLESPACE temp SHRINK SPACE KEEP 1G;
-- Nuclear option (operational): resize the tempfile
ALTER DATABASE TEMPFILE '/u01/oradata/orcl/temp01.dbf' RESIZE 2G;
```

**Avoid dropping/recreating TEMP in production.** If unavoidable, first verify no active temp users (`oracle_temp_by_session`), then create temp2, switch default, drop old — each step needs approval and a maintenance window.

## Best Practices

1. **Alert at 80% used, monitor autoextend limits** — not just current usage.
2. **Gather stats before HWM analysis** — waste estimates are only as good as `num_rows`/`avg_row_len`.
3. **SHRINK COMPACT during business hours, final SHRINK in low traffic.**
4. **Always rebuild indexes after MOVE** — forgetting this is a classic production incident (ORA-01502).
5. **Size TEMP for peak** using `oracle_tablespace_growth_history`.
6. **Include LOB segments in capacity projections** — they can dwarf their parent tables.
7. **Never set MAXSIZE UNLIMITED on production datafiles** — cap with headroom and monitor the file system too.
8. **DELETE does not free space** — it lowers nothing; use TRUNCATE where appropriate or SHRINK/MOVE after mass deletes.

## Common Mistakes

- **Monitoring allocated size instead of used** — `dba_data_files.bytes` is allocation; `oracle_tablespace_usage` computes used correctly.
- **Enabling row movement without checking ROWID-dependent applications.**
- **Dropping a TEMP tablespace with active sessions** — instance instability; check first.
- **Ignoring undo tablespace in space monitoring** — a long-running transaction can balloon it (see `oracle_undo_health`, `oracle_long_transactions`).

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c unless a higher minimum is noted (`MOVE ONLINE` is 12.2+, `SHRINK` on TEMP is 12c+). `dba_tablespace_usage_metrics` is 10g+.

## Sources

- [19c Administrator's Guide — Managing Tablespaces](https://docs.oracle.com/en/database/oracle/oracle-database/19/admin/managing-tablespaces.html)
- [19c Reference — DBA_TABLESPACE_USAGE_METRICS](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/DBA_TABLESPACE_USAGE_METRICS.html)
- [19c SQL Language Reference — ALTER TABLE](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/ALTER-TABLE.html)
