---
name: oracle-dba
description: Oracle DBA operations playbook for agents, driven by the toolbox *oracle_* tool catalog. Use when the user asks to inspect, troubleshoot, diagnose, or health-check an Oracle database — sessions, locks, space, undo/redo, performance tuning (top SQL, AWR, ASH, waits), alert log errors, Data Guard sync, backups, scheduler jobs, security audit (privileges, unified auditing, TDE), or schema discovery. Replaces hand-written SQL with fixed diagnostic tools; covers 19c and older releases unless a tool description notes a higher minimum version.
---

# Oracle DBA Operations

DBA 知识库 + 固定诊断工具集的组合。诊断类查询一律优先调用 `oracle_*` 工具（不需要也不应该现场手写 SQL）；工具不覆盖的场景用 `oracle_execute_sql` 兜底；任何 DML/DDL/ALTER SYSTEM/KILL 只产出"建议 + 需人工确认"，不得直接执行。

## Tool Naming

工具在 toolbox 配置里以基名（如 `oracle_blocking_sessions`）定义，部署后按实例生成带后缀的实际名称（如 `oracle_blocking_sessions_cbspro`）。本文所有工具名均为基名，调用时按当前实例补后缀。带参数的工具用位置绑定 `:1/:2`，参数含义见各工具描述。

## Catalog Overview (86 tools)

| Domain | Tools |
|--------|-------|
| General | `execute_sql`, `instance_overview`, `nondefault_parameters`, `active_alerts` |
| Sessions & locks | `list_active_sessions`, `blocking_sessions`, `session_counts`, `resource_limits`, `cursor_usage`, `session_wait_details` |
| Space | `tablespace_usage` (threshold param, CRITICAL/WARNING/WATCH grade), `datafile_usage`, `temp_usage`, `temp_by_session`, `top_segments`, `segment_type_summary`, `hwm_waste_tables`, `lob_segments`, `tablespace_growth_history` (AWR), `asm_diskgroup_usage`, `fra_usage` |
| Undo / redo / archive | `undo_health`, `long_transactions`, `redo_log_config`, `redo_switches`, `archived_log_inventory` |
| Performance | `top_sql` (sort_by param), `sql_text`, `sql_plan`, `sql_stats_history`, `awr_top_sql`, `ash_top_sql`, `ash_top_events`, `ash_aas_trend`, `ash_blocking`, `ash_session_timeline`, `wait_events`, `session_wait_details`, `hot_datafiles_io`, `instance_load_stats`, `memory_overview`, `memory_advisors`, `top_pga_sessions`, `literal_sql`, `long_running_sql`, `stale_stats`, `autotask_status`, `unindexed_foreign_keys`, `index_usage` (12.2+) |
| Diagnostics | `diag_info`, `alert_log_errors` (12.1+), `block_corruption`, `hm_findings` (11g+), `shared_pool_top` |
| Data Guard | `dg_role_snapshot`, `dg_processes`, `dg_transport_status`, `dg_archive_gap`, `dg_standby_logs`, `dataguard_stats`, `archive_dest_status` |
| Backup | `rman_backup_summary` |
| Jobs | `scheduler_job_failures`, `dba_jobs_broken`, `datapump_jobs` |
| Security audit | `locked_accounts`, `users_with_default_password`, `audit_mode_policies` (12.1+), `audit_failed_logins` (12.1+), `audit_ddl_stream` (12.1+), `sensitive_columns`, `redaction_policies` (12.1+), `tde_wallet_status` (12.1+), `encryption_coverage`, `public_dangerous_grants`, `dangerous_sys_privs`, `dba_role_members`, `user_priv_tree` (11.2+) |
| Discovery | `list_schemas`, `list_tables`, `find_tables`, `describe_table`, `table_constraints`, `table_indexes`, `schema_summary`, `list_invalid_objects`, `sequence_near_max` |

每个工具的 description 都写了"何时用 + 怎么解读 + 配套工具"，调用前先读它。

## Directory Routing

| Topic | File |
|-------|------|
| Data Guard ops, redo/undo management, user management | `admin/` |
| Agent-safe DML, destructive-op guards, idempotency, ORA-error playbook, schema discovery | `agent/` |
| Data Guard architecture, RAC concepts | `architecture/` |
| RMAN basics, backup verification, recovery decision | `backup-recovery/` |
| Online operations, schema migration discipline | `devops/` |
| Alert log / ADR, space management, health monitor, top SQL | `monitoring/` |
| AWR, ASH, waits, plans, stats, memory, indexes | `performance/` |
| Privileges, unified auditing, TDE, redaction, VPD | `security/` |

## Triage Flows

**Slow / degraded instance**
`instance_overview`（版本/启动时长/open mode）→ `ash_aas_trend`（有无突刺、哪类等待）→ `ash_top_events` + `wait_events`（等待结构）→ 按 class 深入：User I/O → `hot_datafiles_io`；Concurrency/锁 → `blocking_sessions` + `ash_blocking`；Commit → `redo_log_config` + `redo_switches`；CPU → `top_sql`(sort_by=CPU)。收尾看 `stale_stats` + `autotask_status`。

**One slow query**
`top_sql` 定位 → `sql_text`（全文）→ `sql_plan`（当前计划）→ `sql_stats_history`（计划是否翻转、何时开始）→ `stale_stats`（统计信息原因）→ 建议（索引/统计/hint），只建议不执行。

**Sessions stuck / lock wait**
`blocking_sessions`（当前链）→ `ash_blocking`（近期模式）→ `session_wait_details`（P1/P2 定位对象）→ 处理建议（优化 SQL 或 kill session，kill 必须人工确认）。

**Space / ORA-0165x**
`tablespace_usage`（min_used_pct=0 全量，看 status 和 has_autoextend）→ `datafile_usage`（是否 MAXSIZE 封顶）→ ASM 存储再看 `asm_diskgroup_usage`。TEMP 类：`temp_usage` → `temp_by_session`（定位会话）。undo 类：`undo_health` → `long_transactions`。收缩/扩容操作看 `monitoring/space-management.md` 的操作章节（人工确认流程）。

**Archive stuck / FRA full**
`fra_usage` → `archived_log_inventory` → `archive_dest_status` → `alert_log_errors`（分钟=120）。DG 环境加 `dg_transport_status`。

**Instance just threw ORA-600 / ORA-7445 / 4031**
`alert_log_errors`（定位时间与错误号）→ `hm_findings`（ADR 健康检查结论）→ `block_corruption`（600 常伴随损坏）→ 4031 加 `shared_pool_top` + `literal_sql`。

**Data Guard lag / switchover check**
主库：`dg_role_snapshot` + `dg_transport_status` + `dg_processes`；备库：`dataguard_stats` + `dg_processes` + `dg_archive_gap` + `dg_standby_logs`。切换前置检查全部通过才建议 switchover（执行人工确认）。

**Backup not running / failed**
`rman_backup_summary`（14 天状态）→ 失败时 `fra_usage` + `alert_log_errors`。RMAN 命令本身看 `backup-recovery/rman-basics.md`（人工执行）。

**Security review**
`dangerous_sys_privs` + `dba_role_members` + `public_dangerous_grants` → `users_with_default_password` + `locked_accounts` → `audit_mode_policies`（审计是否在位）→ `audit_failed_logins` + `audit_ddl_stream` → TDE：`tde_wallet_status` + `encryption_coverage`。整改语句一律人工确认。

**Scheduled jobs failing**
`scheduler_job_failures` + `dba_jobs_broken` → `alert_log_errors` 找失败时段报错 → 涉及对象的用 discovery 工具确认结构。

## Discipline

1. 只读工具随意调用；`oracle_execute_sql` 默认只跑 SELECT。
2. 所有写操作（DML/DDL/ALTER SYSTEM/KILL/SHRINK/重建表空间等）输出为"建议 + 风险 + 人工确认"，绝不在未经确认时执行。
3. 诊断顺序自上而下（cheap → expensive）：先概览后细节，先当前后历史，先工具后 execute_sql。
4. 版本注意：description 标注 12.1+/12.2+/11.2+ 的工具在老库上会因视图不存在而报错——此时改用 execute_sql + 该域文档中的兼容查询。
5. Oracle 标识符默认大写：owner/table_name 参数按库中存储形式传入，不要静默做大小写转换。
