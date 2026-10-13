# Health Monitor and Database Advisors

Oracle's Health Monitor runs structural integrity checks (control file, datafile headers, redo, undo, dictionary) on demand or automatically after critical errors; results are stored in the ADR and visible via `V$HM_*` views. The advisory framework (SQL Tuning, Segment, Memory advisors) complements it with workload-based recommendations.

## Checking Health Monitor Results

Call `oracle_hm_findings` (11g+) → newest runs with status and `damage_description` per finding. Any row is worth reading; findings whose `run_status` is `COMPLETED` with a finding attached are the interesting ones. Also visible via `adrci> SHOW HM_FINDING`.

*No dedicated tool — run via `oracle_execute_sql`:* detailed run status and the check catalog:

```sql
SELECT r.name AS run_name, r.check_name, r.status, r.start_time, r.end_time, r.num_incident
FROM   v$hm_run r
ORDER  BY r.start_time DESC
FETCH FIRST 20 ROWS ONLY;

SELECT c.name AS check_name, p.name AS param_name, p.description, p.default_value
FROM   v$hm_check c JOIN v$hm_check_param p ON c.id = p.check_id
ORDER  BY c.name, p.name;
```

## Built-in Checks — When to Run Them

| Check | Use when | Offline capable |
|-------|----------|-----------------|
| DB Structure Integrity | after crash, restore, storage change; control file/datafile/redo header consistency | Yes (mount) |
| Data Block Integrity | after ORA-01578, storage failure (target specific file/block) | No |
| Redo Integrity | before/after media recovery, redo gaps | Yes |
| Undo Segment Integrity | after unexpected shutdown, ORA-01555/rollback errors | No |
| Transaction Integrity | targeted check of one TXN_ID | No |
| Dictionary Integrity | after patches, failed DDL, dict-related ORA-00600 (slow on big DBs) | No |

## Running Checks (operational — needs approval)

```sql
BEGIN
    DBMS_HM.run_check(
        check_name => 'DB Structure Integrity Check',
        run_name   => 'STRUCT_' || TO_CHAR(SYSDATE, 'YYYYMMDD_HH24MISS'));
END;
/

-- Targeted block check
BEGIN
    DBMS_HM.run_check(
        check_name   => 'Data Block Integrity Check',
        run_name     => 'BLOCK_' || TO_CHAR(SYSDATE, 'YYYYMMDD_HH24MISS'),
        input_params => DBMS_HM.get_run_param('FILE_ID', '5')
                     || DBMS_HM.get_run_param('BLOCK_ID', '128'));
END;
/

-- Transaction check (get XID from v$transaction first)
BEGIN
    DBMS_HM.run_check(
        check_name   => 'Transaction Integrity Check',
        run_name     => 'TXN_CHECK_20260306',
        input_params => DBMS_HM.get_run_param('TXN_ID', '0500.010.000003e8'));
END;
/
```

Text report for a run: `DBMS_HM.get_run_report('<run_name>', 'TEXT')`.

Weekly scheduling example:

```sql
BEGIN
    DBMS_SCHEDULER.create_job(
        job_name        => 'WEEKLY_HM_STRUCT_CHECK',
        job_type        => 'PLSQL_BLOCK',
        job_action      => q'[BEGIN
            DBMS_HM.run_check(check_name => 'DB Structure Integrity Check',
                              run_name   => 'SCHED_STRUCT_' || TO_CHAR(SYSDATE, 'YYYYMMDD'));
        END;]',
        start_date      => TRUNC(NEXT_DAY(SYSDATE, 'SUNDAY')) + INTERVAL '2' HOUR,
        repeat_interval => 'FREQ=WEEKLY; BYDAY=SUN; BYHOUR=2; BYMINUTE=0',
        enabled         => TRUE);
END;
/
```

## Advisors

**Memory advisor** — call `oracle_memory_advisors`: for each candidate size (factor 1.0 = current) read the metric per advisor — SGA: est. DB time; SHARED_POOL: library-cache load time; DB_CACHE: est. physical read factor (lower better); PGA: cache hit % (higher better). Find the knee, don't over-allocate. Current allocation context: `oracle_memory_overview`; per-session PGA hogs: `oracle_top_pga_sessions`.

**Segment advisor** — run it (operational) before deciding SHRINK/MOVE:

```sql
DECLARE
    v_task_name VARCHAR2(100) := 'SEG_ADV_' || TO_CHAR(SYSDATE, 'YYYYMMDD');
    v_object_id NUMBER;
BEGIN
    DBMS_ADVISOR.create_task(advisor_name => 'Segment Advisor', task_name => v_task_name);
    DBMS_ADVISOR.create_object(task_name => v_task_name, object_type => 'SCHEMA',
                               attr1 => '<SCHEMA>', object_id => v_object_id);
    DBMS_ADVISOR.set_task_parameter(task_name => v_task_name,
                                    parameter => 'RECOMMEND_ALL', value => 'TRUE');
    DBMS_ADVISOR.execute_task(task_name => v_task_name);
END;
/
```

*No dedicated tool — run via `oracle_execute_sql`:* read completed Segment Advisor findings:

```sql
SELECT o.owner, o.object_name, o.object_type, f.message, f.more_info
FROM   dba_advisor_findings f
JOIN   dba_advisor_objects   o ON o.task_id = f.task_id AND o.object_id = f.object_id
JOIN   dba_advisor_tasks     t ON t.task_id = o.task_id
WHERE  t.advisor_name = 'Segment Advisor' AND t.status = 'COMPLETED'
ORDER  BY o.owner, o.object_name;
```

Fast heuristic without the advisor: call `oracle_hwm_waste_tables` (min_alloc_mb=100).

**SQL Tuning Advisor** — operational; identify the target first via `oracle_top_sql` / `oracle_awr_top_sql`, then:

```sql
DECLARE
    v_task_name VARCHAR2(100);
BEGIN
    v_task_name := DBMS_SQLTUNE.create_tuning_task(
        sql_id => 'abc123def456g', time_limit => 300,
        task_name => 'TUNE_ABC123', description => 'Tune top CPU query');
    DBMS_SQLTUNE.execute_tuning_task(task_name => 'TUNE_ABC123');
END;
/

SELECT DBMS_SQLTUNE.report_tuning_task('TUNE_ABC123') AS report FROM dual;

-- Accepting a profile changes plans: test in non-prod first (needs approval)
BEGIN
    DBMS_SQLTUNE.accept_sql_profile(task_name => 'TUNE_ABC123', force_match => TRUE);
END;
/
```

## Best Practices

1. **Run Health Monitor after every maintenance window** (patches, imports, storage work) — catches subtle corruption before users do.
2. **Always run DB Structure Integrity after storage/hardware changes.**
3. **Schedule weekly checks and alert on FAILURE findings** — fold `oracle_hm_findings` into the daily health check.
4. **Run Segment Advisor before SHRINK/MOVE** to prioritize; filter out SYS/SYSTEM.
5. **Consult memory advisors before resizing SGA/PGA**; run them during representative peak, not after a quiet night (mixed workloads skew advice).
6. **Name runs with timestamps** (`<TYPE>_YYYYMMDD_HH24MISS`) so history is queryable.
7. **Test SQL profiles/baselines in non-prod** before accepting in production.

## Common Mistakes

- **Running heavy checks (Dictionary Integrity) at peak** — they take locks and reads; use maintenance windows.
- **Finding failures without acting on the recommendations** — worse than not checking.
- **Ignoring ADVISORY findings** — early warnings (segment growth, minor dictionary inconsistency); review weekly.
- **Trusting advisor snapshots from an unrepresentative workload window.**

## Oracle Version Notes (19c vs 26ai)

- Baseline valid for 19c unless a higher minimum is noted; dual-support estates should verify behavior per release update.

## See Also

- [Space Management](space-management.md) — acting on Segment Advisor output (SHRINK/MOVE)
- [Alert Log Analysis](alert-log-analysis.md) — errors that trigger automatic HM runs

## Sources

- [19c PL/SQL Packages — DBMS_HM](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_HM.html)
- [19c PL/SQL Packages — DBMS_SQLTUNE](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_SQLTUNE.html)
- [19c Reference — V$HM_CHECK](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-HM_CHECK.html)
