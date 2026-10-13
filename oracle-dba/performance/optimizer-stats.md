# Optimizer Statistics — Collection, Management, and Tuning

The Cost-Based Optimizer is only as good as its statistics: table (rows, blocks, avg row length), column (NDV, nulls, histograms), index (clustering factor, leaf blocks, blevel) and system stats. `DBMS_STATS` manages them. Stale or missing stats are a leading cause of sudden plan regressions — always check them early in a tuning investigation (`oracle_stale_stats`).

## Monitoring Stats Health — Tools

- `oracle_stale_stats` → tables with `stale_stats='YES'` or never analyzed (user schemas, top 50). Oracle marks stale after ~10% row changes.
- `oracle_autotask_status` → is `auto optimizer stats collection` ENABLED? When disabled, stats rot silently. Check maintenance windows too via execute_sql (`dba_scheduler_windows`).
- Deep-dive per table: *No dedicated tool — run via `oracle_execute_sql`:*

```sql
-- Table/column/index stats for one table
SELECT table_name, num_rows, blocks, avg_row_len, last_analyzed, stale_stats
FROM   dba_tab_statistics
WHERE  owner = 'HR' AND table_name = 'ORDERS';

SELECT column_name, num_distinct, num_nulls, histogram, num_buckets, last_analyzed
FROM   dba_tab_col_statistics
WHERE  owner = 'HR' AND table_name = 'ORDERS';

SELECT index_name, clustering_factor, num_rows, leaf_blocks, blevel, last_analyzed
FROM   dba_ind_statistics
WHERE  owner = 'HR' AND table_name = 'ORDERS';

-- Modification counts driving staleness (lazily flushed; force with
-- DBMS_STATS.FLUSH_DATABASE_MONITORING_INFO first)
SELECT m.table_owner, m.table_name, m.inserts, m.updates, m.deletes,
       ROUND((m.inserts + m.updates + m.deletes) / NULLIF(t.num_rows, 0) * 100, 2) AS pct_modified
FROM   dba_tab_modifications m
JOIN   dba_tables t ON m.table_owner = t.owner AND m.table_name = t.table_name
WHERE  m.table_owner = 'HR'
ORDER  BY pct_modified DESC;

-- Clustering factor: close to blocks = good, close to num_rows = random-I/O index
SELECT i.index_name, i.clustering_factor, t.blocks, t.num_rows,
       ROUND(i.clustering_factor / NULLIF(t.blocks, 0), 1) AS cf_to_blocks_ratio
FROM   dba_indexes i
JOIN   dba_tables  t ON i.owner = t.owner AND i.table_name = t.table_name
WHERE  i.owner = 'HR' AND i.table_name = 'ORDERS'
ORDER  BY i.clustering_factor DESC;
```

High clustering factor → consider reordering the table (CTAS `ORDER BY`) or an IOT; it makes index range scans expensive regardless of the index itself.

## Gathering Statistics (operational — plan-affecting, approval required)

```sql
-- Defaults are right for most cases; AUTO_SAMPLE_SIZE + SIZE AUTO in 11g+
BEGIN
  DBMS_STATS.GATHER_TABLE_STATS(
    ownname => 'HR', tabname => 'EMPLOYEES',
    estimate_percent => DBMS_STATS.AUTO_SAMPLE_SIZE,
    method_opt       => 'FOR ALL COLUMNS SIZE AUTO',
    cascade          => TRUE,
    no_invalidate    => FALSE);   -- invalidate dependent cursors immediately
END;
/

BEGIN
  DBMS_STATS.GATHER_SCHEMA_STATS(ownname => 'HR',
    estimate_percent => DBMS_STATS.AUTO_SAMPLE_SIZE,
    method_opt       => 'FOR ALL COLUMNS SIZE AUTO',
    cascade => TRUE, degree => 4, options => 'GATHER');
END;
/
```

`options` also supports `GATHER STALE` / `GATHER EMPTY` / `LIST STALE`. Gather after bulk loads — a batch that inserts millions of rows makes stats stale immediately.

## Histograms

Capture data skew so the optimizer stops assuming uniform distribution. With `SIZE AUTO` Oracle creates them from workload feedback (col_usage + skew). Caveats:

- Histograms + bind variables can destabilize plans (bind peeking on a skewed column).
- Column groups (extended stats) fix correlated-column misestimates; expression stats fix function-wrapped predicates. **Always gather again after creating extended stats.**

## Locking, Export, Pending (operational)

- `DBMS_STATS.LOCK_TABLE_STATS` — for reference/lookup tables that rarely change.
- Export stats (`DBMS_STATS.EXPORT_TABLE_STATS` into a stats table) before upgrades/major changes so the previous state is restorable when plans regress.
- Pending statistics (11g+): `SET_TABLE_PREFS(... 'PUBLISH','FALSE')` → gather → test with `ALTER SESSION SET OPTIMIZER_USE_PENDING_STATISTICS = TRUE` → `PUBLISH_PENDING_STATS` or `DELETE_PENDING_STATS`. The safe way to trial new stats in production.

## Best Practices

- **Defaults first**: `AUTO_SAMPLE_SIZE` + `FOR ALL COLUMNS SIZE AUTO` beat fixed samples in 11g+.
- **Incremental stats for big partitioned tables**: `SET_TABLE_PREFS(... 'INCREMENTAL','TRUE')` — avoids rescanning unchanged partitions.
- **Check `oracle_stale_stats` before large batch jobs.**
- **Never disable autostats globally without a replacement job** — the regression debt arrives weeks later.
- **Gather after creating extended stats** — they contain no data until you do.

## Common Mistakes

| Mistake | Impact | Fix |
|---|---|---|
| New tables never analyzed | dynamic sampling guesswork | gather after initial load |
| Fixed sample sizes (1%, 10%) | inaccurate on skewed data | `AUTO_SAMPLE_SIZE` |
| `NO_INVALIDATE => TRUE` habit | old plans persist against new stats | default (deferred) or FALSE |
| Disabling autostats "to stop plan flips" | slow-motion regression across the estate | replace with a tuned custom job |
| Histograms on bind-heavy skewed columns | plan instability | `SIZE 1` for those columns; fix app binds |

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c unless noted; `REAL-Time` stats (19c) and 26ai additions follow their own defaults — verify per release update.

## See Also

- [Explain Plan](explain-plan.md) — how stats errors show up as E-Rows/A-Rows divergence
- [Index Strategy](index-strategy.md) — clustering factor and index choice

## Sources

- [19c SQL Tuning Guide](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgsql/)
- [19c PL/SQL Packages — DBMS_STATS](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_STATS.html)
- [19c Reference — DBA_TAB_STATISTICS](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/DBA_TAB_STATISTICS.html)
