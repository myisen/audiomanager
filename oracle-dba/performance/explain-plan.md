# Explain Plan — Execution Plan Analysis

An execution plan is the optimizer's chosen operation sequence. Four ways to capture one: `EXPLAIN PLAN` (estimate, no execution), `DBMS_XPLAN.DISPLAY_CURSOR` (the plan that actually ran), `DBMS_XPLAN.DISPLAY_AWR` (historical, from AWR), AUTOTRACE (plan + stats in SQL*Plus).

Agent flow: `oracle_top_sql` / `oracle_awr_top_sql` pick the sql_id → `oracle_sql_plan` shows the cached plan steps → `oracle_sql_stats_history` shows when the plan hash changed → the `DBMS_XPLAN` calls below give the formatted, annotated plan.

## Getting the Plan

**Cached plan structure** — call `oracle_sql_plan` (sql_id): operation/options per step with cost/cardinality, one block per child cursor. Multiple children = multiple plans in play.

**Formatted plan (operational, read-only)** — via `oracle_execute_sql`:

```sql
-- Find the sql_id first
SELECT sql_id, plan_hash_value, sql_text
FROM   v$sql
WHERE  sql_text LIKE '%orders%' AND sql_text NOT LIKE '%v$sql%'
ORDER  BY last_active_time DESC
FETCH FIRST 10 ROWS ONLY;

-- The plan that actually ran (preferred over EXPLAIN PLAN)
SELECT * FROM TABLE(
  DBMS_XPLAN.DISPLAY_CURSOR(
    sql_id        => 'abc123xyz789',
    cursor_child_no => NULL,          -- NULL = most recent child
    format        => 'TYPICAL +IOSTATS LAST +PEEKED_BINDS'));

-- Historical plan from AWR (23ai+: use DISPLAY_WORKLOAD_REPOSITORY instead)
SELECT * FROM TABLE(
  DBMS_XPLAN.DISPLAY_AWR(sql_id => 'abc123xyz789', format => 'TYPICAL'));

-- Estimated plan without executing
EXPLAIN PLAN SET STATEMENT_ID = 'MY_QUERY' FOR
SELECT * FROM orders WHERE status = 'PENDING';
SELECT * FROM TABLE(DBMS_XPLAN.DISPLAY(NULL, 'MY_QUERY', 'TYPICAL'));
```

`EXPLAIN PLAN` can lie when binds/adaptive plans/stale stats are involved — always prefer `DISPLAY_CURSOR` for production SQL.

To get actual row counts per operation, collect statistics first: `ALTER SESSION SET STATISTICS_LEVEL = ALL` in your session, or add the `/*+ GATHER_PLAN_STATISTICS */` hint, then format `'ALLSTATS LAST'`.

## Reading the Plan

```
| Id | Operation                     | Name        | Rows | Cost (%CPU)|
|  0 | SELECT STATEMENT              |             |  500 |  142   (2) |
|  1 |  HASH JOIN                    |             |  500 |  142   (2) |
|  2 |   TABLE ACCESS FULL           | DEPARTMENTS |   27 |    3   (0) |
|* 3 |   TABLE ACCESS BY INDEX ROWID | EMPLOYEES   |  500 |  139   (1) |
|* 4 |    INDEX RANGE SCAN           | EMP_DEPT_IX |  503 |    2   (0) |

Predicate Information:
   3 - filter("E"."SALARY">10000)          -- applied after fetching
   4 - access("E"."DEPARTMENT_ID"="D"."DEPARTMENT_ID")  -- drives the index lookup
```

- Read **inside-out, bottom-up** (most indented first).
- `Rows` is an *estimate*; with `ALLSTATS LAST`, compare **E-Rows vs A-Rows** — the first step where they diverge 10x+ is where the plan went wrong.
- `A-Time`/`Buffers` are cumulative (parents include children).
- Check `access` vs `filter` predicates: access predicates match your index structure; filters applied late on large row sources are expensive.

### Access Paths and Joins

| Operation | Good when |
|---|---|
| TABLE ACCESS FULL | most of the table needed; small table |
| BY INDEX ROWID + RANGE SCAN | selective predicate, decent clustering |
| INDEX UNIQUE SCAN | PK/unique lookup |
| INDEX FAST FULL SCAN | index covers all columns |
| NESTED LOOPS | small outer, selective inner index |
| HASH JOIN | large sets, no useful index |
| MERGE JOIN | inputs pre-sorted |

## Symptom → Fix Patterns

| Symptom in plan | Usual cause | Fix |
|---|---|---|
| E-Rows ≪ A-Rows at a step | stale/missing stats, bind peeking | `oracle_stale_stats` → gather; extended stats; verify |
| FTS where index expected | function on indexed column, implicit type conversion, leading `%` wildcard | function-based index; fix types; app redesign |
| NESTED LOOPS over huge outer | bad cardinality | stats first, then hints/SPM |
| FILTER with correlated subquery per row | subquery not unnested | rewrite with analytic JOIN |
| SORT-MERGE instead of HASH on large join | stale system stats | gather sys stats; test `/*+ USE_HASH */` |

Structural fixes (function-based index, reorg) are operational changes — approval required.

## Locking a Good Plan (SPM — operational)

```sql
DECLARE
  l_plans PLS_INTEGER;
BEGIN
  l_plans := DBMS_SPM.LOAD_PLANS_FROM_CURSOR_CACHE(
    sql_id => 'abc123xyz789', plan_hash_value => 1234567890);
  DBMS_OUTPUT.PUT_LINE('Plans loaded: ' || l_plans);
END;
/
```

Grant `ADMINISTER SQL MANAGEMENT OBJECT` to trusted DBAs only.

## Security Considerations (condensed)

- Plans reveal schema/index structure — treat as sensitive; restrict `v_$sql_plan`/`v_$sqlarea` grants to tuning roles, obfuscate names when sharing externally.
- `+PEEKED_BINDS` output can show **actual bind values** (potentially card numbers, PHI) — sanitize before sharing; compliance frameworks (PCI/HIPAA/GDPR) expect this access controlled. Audit with:

```sql
CREATE AUDIT POLICY plan_access_audit
  ACTIONS SELECT ON SYS.V_$SQL_PLAN, SELECT ON SYS.V_$SQLAREA;
AUDIT POLICY plan_access_audit;
```

- The analyzed SQL itself must use bind variables (injection + plan stability); see the guardrails in [agent/safe-dml-patterns.md](../agent/safe-dml-patterns.md).

## Best Practices

- **`DISPLAY_CURSOR` over `EXPLAIN PLAN`** for production debugging; add `GATHER_PLAN_STATISTICS` when you can control the session.
- **Find the first E-Rows/A-Rows divergence** — fix there, not at the top of the tree.
- **Cost is a model, not milliseconds** — verify with `oracle_sql_stats_history` or actual runs.
- **Prefer stats/index fixes over hints**; lock the good plan with SPM instead of hinting code.

## Common Mistakes

- Trusting `EXPLAIN PLAN` for bind-heavy SQL.
- Reading cost as speed.
- Adding hints to production code — brittle; use statistics + SPM.
- Cumulative A-Time misread as per-step time (subtract children).

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c. `DISPLAY_AWR` deprecated from 23ai in favor of `DISPLAY_WORKLOAD_REPOSITORY`.

## See Also

- [Top SQL](../monitoring/top-sql-queries.md) — finding the sql_id worth planning
- [ASH Analysis](ash-analysis.md) — what the session waited on while running it

## Sources

- [19c SQL Tuning Guide](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgsql/)
- [19c PL/SQL Packages — DBMS_XPLAN](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_XPLAN.html)
- [19c PL/SQL Packages — DBMS_SPM](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_SPM.html)
