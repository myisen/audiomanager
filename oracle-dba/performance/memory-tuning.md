# Memory Tuning — SGA, PGA, and Oracle Memory Management

Two management modes: **AMM** (`MEMORY_TARGET`, SGA+PGA together — only for small systems; incompatible with Linux HugePages) and **ASMM** (`SGA_TARGET` + `PGA_AGGREGATE_TARGET` separately — the production default on Linux). All sizing changes below are `ALTER SYSTEM` operational changes requiring approval.

## Quick Health Check — Tools

1. `oracle_memory_overview` — SGA components (v$sga) + PGA statistics (v$pgastat) in MB: total PGA inuse vs target, over-allocation, cache hit percentage (aim > 90%).
2. `oracle_memory_advisors` — all four advisors in one result: for each candidate size (factor 1.0 = current) the expected metric — SGA: est. DB time; SHARED_POOL: library-cache load time; DB_CACHE: est. physical read factor (lower better); PGA: cache hit % (higher better). Find the knee; doubling size for <10% gain is waste.
3. `oracle_top_pga_sessions` — the 20 largest per-session PGA consumers (big sorts, PL/SQL collections).
4. `oracle_shared_pool_top` + `oracle_literal_sql` — the ORA-04031 pair: biggest allocations + literal-SQL families causing churn.
5. `oracle_instance_load_stats` — `sorts (disk)` vs `sorts (memory)` (disk sorts should be a tiny fraction), hard vs total parses.

## Shared Pool

Cache for parsed cursors and dictionary data. Two failure modes:

- **ORA-04031 (exhaustion/fragmentation)**: check `oracle_shared_pool_top` — runaway subsystems, tiny free memory. Then `oracle_literal_sql`: literal SQL variants force hard parses and fragment the pool. The real fix is bind variables in the app; `CURSOR_SHARING=FORCE` is an emergency workaround (operational, has side effects with histograms).
- **Undersizing**: library-cache reloads/misses — visible via advisor metrics and parse ratios in `oracle_instance_load_stats`.

Sizing advice: `oracle_memory_advisors` SHARED_POOL rows. Large pool: size for RMAN/parallel/MTS workloads (`LARGE_POOL_SIZE` minimum in ASMM).

## PGA

Per-session private memory: sort/hash work areas, cursor state.

```sql
-- Operational: automatic PGA management (the default; 12c+ hard limit)
ALTER SYSTEM SET WORKAREA_SIZE_POLICY  = AUTO;
ALTER SYSTEM SET PGA_AGGREGATE_TARGET  = 4G;
ALTER SYSTEM SET PGA_AGGREGATE_LIMIT   = 8G;   -- runaway-query guard, ~2x target
```

- **Sizing**: `oracle_memory_advisors` PGA rows — look for the point where cache-hit% plateaus and over-allocation disappears.
- **Sort spills to TEMP** (work area too small → disk, dramatic slowdown): `oracle_temp_by_session` shows who is spilling now; `oracle_instance_load_stats` gives the memory/disk sort ratio; `oracle_tablespace_growth_history` shows TEMP pressure over weeks. Fix: bigger PGA target or better SQL (smaller intermediate sets).
- **Runaway session**: `oracle_top_pga_sessions` → the pga_max_mb column tells you how far a single session has grown; `PGA_AGGREGATE_LIMIT` will error such sessions out instead of starving the instance.

## SGA

- **Buffer cache**: `oracle_memory_advisors` DB_CACHE rows (est. physical read factor per size). `free buffer waits` in `oracle_wait_events` = cache too small or DBWR too slow.
- **ASMM component shuffling** — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT component, ROUND(current_size/1048576) AS current_mb,
       ROUND(min_size/1048576) AS min_mb, last_oper_type
FROM   v$sga_dynamic_components
ORDER  BY current_size DESC;
```

Frequent SHRINK/RESIZE operations on a component mean `SGA_TARGET` is too small for the workload.

```sql
-- Operational: enable ASMM (recommended production setup on Linux)
ALTER SYSTEM SET MEMORY_TARGET          = 0;
ALTER SYSTEM SET SGA_TARGET             = 16G;
ALTER SYSTEM SET PGA_AGGREGATE_TARGET   = 4G;
ALTER SYSTEM SET DB_CACHE_SIZE          = 4G;    -- optional minimums
ALTER SYSTEM SET SHARED_POOL_SIZE       = 1G;
```

- **AMM** (`MEMORY_TARGET`/`MEMORY_MAX_TARGET` in spfile): convenient but no HugePages and less predictable — keep for small/dev systems only.
- **HugePages (Linux, large SGAs)**: set `vm.nr_hugepages` ≈ SGA_MB/2 + ~10% in `/etc/sysctl.conf`; verify with `grep HugePages /proc/meminfo`. Disable AMM first (useLegacyVMemHierarchy → use `USE_LARGE_PAGES=ONLY` in 19c+).

## Tuning Workflow

1. `oracle_memory_overview` — current allocation and PGA vs target.
2. `oracle_wait_events` / `oracle_ash_top_events` — memory-shaped waits? (`free buffer waits`, `shared pool` latches/mutex, `direct path read temp`)
3. Symptom-specific: ORA-04031 → `oracle_shared_pool_top` + `oracle_literal_sql`; temp spills → `oracle_temp_by_session`; PGA hog → `oracle_top_pga_sessions`.
4. `oracle_memory_advisors` — quantify what a resize would buy.
5. Propose the `ALTER SYSTEM` change with factor evidence; apply on approval (most are dynamic; note SPFILE-only ones).

## Best Practices

- **Prefer ASMM + HugePages on production Linux**; AMM only for small/dev systems.
- **Fix literal SQL before growing the shared pool** — `oracle_literal_sql` first, resize second.
- **Set `PGA_AGGREGATE_LIMIT`** on 12c+ so one runaway query cannot take the instance down.
- **Advisor data reflects the recent workload** — run during representative peak.

## Common Mistakes

- Growing SGA blindly until the OS swaps — advisor knees exist for a reason; monitor OS memory too.
- Blaming storage for `log file sync` when the real cause is per-row commits; blaming PGA when sorts spill due to one awful SQL.
- Enabling HugePages while AMM is still active — silently no HugePages.
- Treating `cache hit percentage` as a goal — it is a symptom, not a target.

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c unless noted. PGA_AGGREGATE_LIMIT is 12c+; manual work-area policies (`SORT_AREA_SIZE`) are legacy.

## See Also

- [Wait Events](wait-events.md) — memory-shaped waits in context
- [Space Management](../monitoring/space-management.md) — TEMP sizing and reclamation

## Sources

- [19c Performance Tuning Guide — Memory Configuration](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgdba/)
- [19c Reference — V$PGASTAT](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-PGASTAT.html)
- [19c Reference — V$SGA_TARGET_ADVICE](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-SGA_TARGET_ADVICE.html)
