# Index Strategy — Oracle Index Types and Usage

Indexes trade DML cost and space for read speed. Choose the type deliberately, verify actual usage with tools before dropping, and never rebuild on a schedule "just because".

## Index Types (when to use)

- **B-Tree** — the default: equality/range predicates on medium-high cardinality columns. Not for very low cardinality columns or tiny tables where FTS is cheaper.
- **Bitmap** — low-cardinality columns in data-warehouse, mostly-read workloads. **Avoid in OLTP**: row-level locking on the bitmap makes concurrent DML serialize.
- **Function-Based (FBI)** — for predicates wrapping columns (`UPPER(name)`, expressions). Pre-11g needs `QUERY_REWRITE_ENABLED`; gather stats on the virtual column after creation.
- **Composite** — order columns by: equality predicates first, then range; most selective first helps but equality-then-range dominates. A composite (a,b) does not help predicates on b alone (unless skip-scan applies: low-cardinality leading column).
- **Covering index** — include all queried columns for index-only scans (no table access).
- **Invisible index (11g+)** — `CREATE INDEX ... INVISIBLE` / `ALTER INDEX ... INVISIBLE`: test-drop safely; optimizer ignores it but it still costs DML. Also the fast rollback path after a bad drop decision.

## Verifying Actual Usage — Tools

- `oracle_index_usage` (12.2+, owner LIKE param) → `total_access_count` and `last_used` per index, sorted least-used first. Counters reset on restart — interpret together with uptime (`oracle_instance_overview`). Never-read indexes are drop candidates (make them INVISIBLE first for a full workload cycle).
- `oracle_unindexed_foreign_keys` (owner LIKE param) → enabled FKs whose leading column has no index. **Every hit matters**: parent DML takes a brief full-table lock on the child when the FK is unindexed, and parent→child navigation full-scans.
- Pre-12c manual monitoring (only a TRUE/FALSE used flag — no frequency):

```sql
ALTER INDEX emp_salary_ix MONITORING USAGE;   -- run workload...
SELECT index_name, monitoring, used, start_monitoring, end_monitoring
FROM   v$object_usage WHERE index_name = 'EMP_SALARY_IX';
ALTER INDEX emp_salary_ix NOMONITORING USAGE;
```

## Maintenance: Rebuild vs Coalesce (operational — approval required)

```sql
ALTER INDEX emp_salary_ix COALESCE;                        -- merge leaf blocks in place, cheap, online-safe
ALTER INDEX emp_salary_ix REBUILD ONLINE;                  -- full rebuild; fixes height/CF/storage, reads all table data
ALTER INDEX emp_dept_sal_ix REBUILD COMPRESS 1 ONLINE;     -- compress repeated leading values
```

| Scenario | Recommendation |
|---|---|
| many deletes → leaf waste | COALESCE |
| clustering factor badly degraded | reorganize the **table** instead |
| move tablespace / change storage | REBUILD |
| periodic "just in case" rebuild | don't — healthy indexes don't need it (10g+) |

Validate before deciding (session-level `index_stats`): *No dedicated tool — run via `oracle_execute_sql`:*

```sql
ANALYZE INDEX emp_salary_ix VALIDATE STRUCTURE;
SELECT name, height, lf_rows, lf_blks, del_lf_rows,
       ROUND(del_lf_rows / NULLIF(lf_rows, 0) * 100, 2) AS pct_deleted
FROM   index_stats;    -- > 20-30% deleted → rebuild may help
```

Check current index state first with `oracle_table_indexes` (owner, table) — UNUSABLE status means DML is already failing with ORA-01502.

## Best Practices

- **Drop decisions need evidence**: `oracle_index_usage` over a full business cycle (counters reset on restart), then INVISIBLE before DROP.
- **Index FKs** unless the parent is strictly insert-only — run `oracle_unindexed_foreign_keys` as a periodic check.
- **Compress composite indexes with repeated leading columns.**
- **Gather stats after creating/rebuilding** indexes (see [optimizer-stats](optimizer-stats.md)).

## Common Mistakes

- Adding indexes "to be safe" — every index taxes every INSERT/UPDATE/DELETE.
- Bitmap indexes in OLTP — serialization under concurrent DML.
- Scheduled rebuilds of healthy indexes — wasted maintenance windows; validate first.
- Trusting pre-12c MONITORING USAGE's used flag — one hit per month reads the same as a million.

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c. Automatic usage tracking (`dba_index_usage`) is 12.2+; on older releases use MONITORING USAGE.

## See Also

- [Optimizer Stats](optimizer-stats.md) — clustering factor interpretation
- [Explain Plan](explain-plan.md) — verifying the index is actually chosen

## Sources

- [19c SQL Tuning Guide — Indexes](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgsql/)
- [19c Reference — DBA_INDEX_USAGE](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/DBA_INDEX_USAGE.html)
- [19c Reference — V$OBJECT_USAGE](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-OBJECT_USAGE.html)
