# Oracle Database Auditing

## Overview

Database auditing records who did what, when, and from where — a foundational control for compliance, forensic investigation, and insider-threat detection. Since Oracle 12c the primary and recommended mechanism is **Unified Auditing**, which consolidates standard auditing, fine-grained auditing, and SYS auditing into one framework: every record lands in `UNIFIED_AUDIT_TRAIL` and is managed through `CREATE AUDIT POLICY` / `AUDIT POLICY` / `NOAUDIT` statements.

| Feature | Traditional (pre-12c) | Unified Auditing (12c+) |
|---|---|---|
| Storage | `AUD$` table or OS files | `AUDSYS` schema (secure) |
| Configuration | Multiple parameters | `CREATE AUDIT POLICY` |
| SYS auditing | Separate | Integrated |
| Querying | Multiple views | `UNIFIED_AUDIT_TRAIL` |

### Checking the Auditing Mode

- Call `oracle_audit_mode_policies` (12.1+) → shows whether unified auditing is active and which policies are enabled for which users → if coverage is limited to defaults (or nothing), propose the baseline policies below.
- Call `oracle_nondefault_parameters` → check `audit_trail` → in pure unified auditing it is ignored; in mixed mode it still routes traditional records.

---

## Enabling Pure Unified Auditing

> **Operational — preconditions / risk / confirm:** relinking requires OS access as the `oracle` user and a full database restart (outage). Confirm the maintenance window and take a backup first. Mixed mode (default) is adequate for most estates; relink only when pure unified auditing is a hard requirement.

```bash
# On the database server OS (as oracle OS user):
cd $ORACLE_HOME/rdbms/lib
make -f ins_rdbms.mk uniaud_on ioracle

# After relinking, restart the database
sqlplus / as sysdba
SHUTDOWN IMMEDIATE;
STARTUP;
```

---

## Creating Unified Audit Policies

> **Operational — preconditions / risk / confirm:** requires `AUDIT_ADMIN` or `AUDIT SYSTEM` privilege. Over-broad policies can flood SYSAUX — scope policies deliberately and check volume after enabling.

Basic syntax:

```sql
CREATE AUDIT POLICY policy_name
  [PRIVILEGES privilege_list]
  [ACTIONS action_list]
  [WHEN 'condition_expression' EVALUATE PER SESSION|INSTANCE]
  [CONTAINER = CURRENT|ALL];
```

Audit powerful privilege use:

```sql
CREATE AUDIT POLICY audit_dba_privs
  PRIVILEGES CREATE USER, DROP USER, ALTER USER,
             GRANT ANY PRIVILEGE, GRANT ANY ROLE,
             CREATE ANY TABLE, DROP ANY TABLE,
             AUDIT SYSTEM;

AUDIT POLICY audit_dba_privs;                          -- all users
AUDIT POLICY audit_dba_privs BY hr_admin, sys_admin;   -- specific users only
AUDIT POLICY audit_dba_privs WHENEVER NOT SUCCESSFUL;  -- failures only
```

Audit object actions and logins:

```sql
CREATE AUDIT POLICY audit_salary_changes
  ACTIONS SELECT, INSERT, UPDATE, DELETE ON hr.employees;
AUDIT POLICY audit_salary_changes;

CREATE AUDIT POLICY audit_schema_ddl
  ACTIONS CREATE TABLE, ALTER TABLE, DROP TABLE,
          CREATE INDEX, DROP INDEX,
          CREATE VIEW, DROP VIEW,
          CREATE PROCEDURE, ALTER PROCEDURE, DROP PROCEDURE;
AUDIT POLICY audit_schema_ddl BY hr;

CREATE AUDIT POLICY audit_connections
  ACTIONS LOGON, LOGOFF;
AUDIT POLICY audit_connections;
```

Conditional auditing (`WHEN` clause) filters to exactly the cases of interest and dramatically reduces volume:

```sql
-- Only audit salary access outside business hours
CREATE AUDIT POLICY audit_after_hours_salary
  ACTIONS SELECT ON hr.employees
  WHEN 'TO_NUMBER(TO_CHAR(SYSDATE,''HH24'')) NOT BETWEEN 8 AND 18'
  EVALUATE PER SESSION;
AUDIT POLICY audit_after_hours_salary;

-- Audit access from outside the corporate network
CREATE AUDIT POLICY audit_external_access
  ACTIONS SELECT, INSERT, UPDATE, DELETE ON payments.transactions
  WHEN 'SYS_CONTEXT(''USERENV'',''IP_ADDRESS'') NOT LIKE ''10.%'''
  EVALUATE PER SESSION;
AUDIT POLICY audit_external_access;
```

---

## Auditing Privileged Users (SYS and SYSDBA)

> **Operational — preconditions / risk / confirm:** `audit_sys_operations` needs an SPFILE change and restart; a policy with `ACTIONS ALL` generates very high volume — confirm audit storage sizing before enabling.

```sql
-- SYS-level auditing (OS file or AUDSYS); requires restart
ALTER SYSTEM SET audit_sys_operations = TRUE SCOPE = SPFILE;

-- Unified auditing: all actions taken while connected AS SYSDBA
CREATE AUDIT POLICY audit_sysdba_actions
  ACTIONS ALL
  WHEN 'SYS_CONTEXT(''USERENV'',''ISDBA'') = ''TRUE'''
  EVALUATE PER SESSION;
AUDIT POLICY audit_sysdba_actions;

-- All actions by named DBA accounts
CREATE AUDIT POLICY audit_named_dbas ACTIONS ALL;
AUDIT POLICY audit_named_dbas BY dba_user1, dba_user2;
```

---

## Fine-Grained Auditing (DBMS_FGA)

FGA audits SELECTs referencing specific columns and captures the actual SQL text. In unified auditing mode FGA records appear in `UNIFIED_AUDIT_TRAIL.SQL_TEXT`. (The `audit_trail` parameter for FGA is desupported in 23ai — records always go to the unified trail.)

> **Operational — preconditions / risk / confirm:** a handler procedure writes to an application table that must already exist; test on a low-volume column first.

```sql
-- Audit SELECT on sensitive columns, capturing the query
BEGIN
  DBMS_FGA.ADD_POLICY(
    object_schema   => 'HR',
    object_name     => 'EMPLOYEES',
    policy_name     => 'AUDIT_SALARY_ACCESS',
    audit_column    => 'SALARY,COMMISSION_PCT',  -- fire only when referenced
    audit_condition => NULL,                     -- NULL = always audit
    statement_types => 'SELECT',
    enable          => TRUE
  );
END;
/

-- Fire only for high salaries
BEGIN
  DBMS_FGA.ADD_POLICY(
    object_schema   => 'HR', object_name => 'EMPLOYEES',
    policy_name     => 'AUDIT_HIGH_SALARY_ACCESS',
    audit_column    => 'SALARY',
    audit_condition => 'SALARY > 100000',
    statement_types => 'SELECT'
  );
END;
/

-- Handler variant: set handler_schema / handler_module to a procedure that
-- records an alert row whenever the policy fires.

-- Manage
EXEC DBMS_FGA.DISABLE_POLICY('HR', 'EMPLOYEES', 'AUDIT_SALARY_ACCESS');
EXEC DBMS_FGA.DROP_POLICY('HR', 'EMPLOYEES', 'AUDIT_SALARY_ACCESS');
```

---

## Querying UNIFIED_AUDIT_TRAIL

Key columns: `EVENT_TIMESTAMP`, `DBUSERNAME`, `OS_USERNAME`, `USERHOST`, `UNIFIED_AUDIT_POLICIES`, `ACTION_NAME`, `OBJECT_SCHEMA`, `OBJECT_NAME`, `SQL_TEXT` (CLOB), `RETURN_CODE`, `AUTHENTICATION_TYPE`, `SYSTEM_PRIVILEGE_USED`.

> **Note:** `UNIFIED_AUDIT_TRAIL` has **no** `CLIENT_IP` column — client IP is embedded in `AUTHENTICATION_TYPE`. There is no `AUTHENTICATION_PRIVILEGE` column — use `SYSTEM_PRIVILEGE_USED` for SYSDBA/SYSOPER usage.

Routine checks via dedicated tools:

- Failed logons (brute force): call `oracle_audit_failed_logins` (param `hours`, 12.1+) → reviews ORA-01017/28000 events with user, host, and client details → if failures cluster on one account or source, trigger the lockout/response procedure.
- DDL and GRANT stream: call `oracle_audit_ddl_stream` (param `days`, 12.1+) → reviews recent GRANT/REVOKE/DDL activity → reconcile unexpected changes against change tickets.
- Policy coverage: call `oracle_audit_mode_policies` → compare enabled policies against the intended set.

Deeper forensics — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
-- Who accessed salary data today
SELECT event_timestamp, dbusername, userhost, action_name, sql_text
FROM   unified_audit_trail
WHERE  object_name = 'EMPLOYEES' AND object_schema = 'HR'
AND    unified_audit_policies LIKE '%SALARY%'
AND    event_timestamp > TRUNC(SYSDATE)
ORDER  BY event_timestamp DESC;

-- All SYSDBA activity in the last 7 days
SELECT event_timestamp, dbusername, userhost, action_name,
       object_name, sql_text, system_privilege_used
FROM   unified_audit_trail
WHERE  (dbusername = 'SYS' OR system_privilege_used LIKE '%SYSDBA%')
AND    event_timestamp > SYSDATE - 7
ORDER  BY event_timestamp DESC;

-- Failed access attempts (exclude NOT FOUND 1403)
SELECT event_timestamp, dbusername, action_name, object_name,
       return_code, sql_text
FROM   unified_audit_trail
WHERE  return_code NOT IN (0, 1403)
AND    event_timestamp > SYSDATE - 1
ORDER  BY event_timestamp DESC;

-- Top 20 most active users (volume baseline / anomaly detection)
SELECT dbusername, COUNT(*) event_count
FROM   unified_audit_trail
WHERE  event_timestamp > SYSDATE - 7
GROUP  BY dbusername
ORDER  BY event_count DESC
FETCH FIRST 20 ROWS ONLY;
```

---

## Managing Audit Policies

> **Operational — preconditions / risk / confirm:** `NOAUDIT`/`DROP AUDIT POLICY` silently reduce audit coverage — confirm the requester understands the compliance impact. Purging audit records is irreversible: archive first, use dual control.

```sql
-- Disable / drop
NOAUDIT POLICY audit_salary_changes;
NOAUDIT POLICY audit_salary_changes BY specific_user;
DROP AUDIT POLICY audit_salary_changes;

-- Purge records older than 90 days: set archive timestamp, then clean
BEGIN
  DBMS_AUDIT_MGMT.SET_LAST_ARCHIVE_TIMESTAMP(
    audit_trail_type  => DBMS_AUDIT_MGMT.AUDIT_TRAIL_UNIFIED,
    last_archive_time => SYSTIMESTAMP - INTERVAL '90' DAY);
END;
/
BEGIN
  DBMS_AUDIT_MGMT.CLEAN_AUDIT_TRAIL(
    audit_trail_type        => DBMS_AUDIT_MGMT.AUDIT_TRAIL_UNIFIED,
    use_last_arch_timestamp => TRUE);
END;
/

-- Recurring purge job
BEGIN
  DBMS_AUDIT_MGMT.CREATE_PURGE_JOB(
    audit_trail_type           => DBMS_AUDIT_MGMT.AUDIT_TRAIL_UNIFIED,
    audit_trail_purge_interval => 24,
    audit_trail_purge_name     => 'DAILY_AUDIT_PURGE',
    use_last_arch_timestamp    => TRUE);
END;
/
```

Who can manage the audit trail — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT grantee FROM dba_sys_privs WHERE privilege = 'AUDIT SYSTEM'
UNION
SELECT grantee FROM dba_role_privs WHERE granted_role = 'AUDIT_ADMIN';
```

---

## Audit Trail Size and Location

The unified trail lives in the `AUDSYS` schema in `SYSAUX` by default; move it to a dedicated tablespace for high-volume estates.

Sizing check — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT ROUND(SUM(bytes)/1024/1024/1024, 2) AS size_gb
FROM   dba_segments
WHERE  owner = 'AUDSYS';
```

> **Operational:** moving the trail (`DBMS_AUDIT_MGMT.SET_AUDIT_TRAIL_LOCATION` to a pre-created tablespace) briefly affects audit writes — schedule it and confirm the target tablespace exists first.

---

## Best Practices

1. **Always audit authentication events** — logon failures are the primary brute-force indicator; successes establish the anomaly baseline.
2. **Audit all DDL** in application schemas — changes can destroy evidence or create backdoors.
3. **Audit all privilege and role grants** — SOX/PCI review requirement.
4. **Use `DBMS_FGA` for sensitive column access** when you need the actual SQL text.
5. **Ship audit records to a SIEM or separate store** — a DBA who can delete records can cover their tracks.
6. **Do not over-audit SELECT on busy tables** — use FGA conditions to target what matters.
7. **Review policies quarterly**; remove stale ones as the application evolves.
8. **Test that policies fire** — perform the audited action and re-check `UNIFIED_AUDIT_TRAIL` (or `oracle_audit_ddl_stream`).

## Common Mistakes and How to Avoid Them

**Auditing everything by default** — `AUDIT ALL STATEMENTS;` generates millions of records per hour on busy OLTP. Audit strategically: privilege use, DDL, sensitive data access.

**DBAs purging their own audit records** — keep `AUDIT_ADMIN` out of regular DBA hands, require dual control for purge, and periodically review the `AUDIT SYSTEM`/`AUDIT_ADMIN` holder list above.

**Purging before archiving** — export to a SIEM/archive store first, then `CLEAN_AUDIT_TRAIL` with `use_last_arch_timestamp => TRUE`.

**Policies without SQL text** — for "what exactly did they query", add an FGA policy; `SQL_TEXT` lands in `UNIFIED_AUDIT_TRAIL`.

## Compliance Quick Reference

- **SOX:** financial data access, all privilege grants, and DBA activity logged; 7-year retention; tamper-protected records.
- **PCI-DSS Req. 10:** audit all access to cardholder data; capture time/user/event/outcome; protect and retain ≥ 1 year. PCI baseline policy:

```sql
CREATE AUDIT POLICY pci_cardholder_audit
  ACTIONS SELECT, INSERT, UPDATE, DELETE ON payments.card_data,
  ACTIONS SELECT, INSERT, UPDATE, DELETE ON payments.transactions;
AUDIT POLICY pci_cardholder_audit;
```

- **HIPAA:** 45 CFR 164.312(b) — record and examine activity on systems holding PHI; include user ID, date/time, access type; review regularly; retain ≥ 6 years.

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance is valid for Oracle Database 19c unless a newer minimum is explicitly called out.
- Features marked 21c/23c/23ai should be treated as 26ai-capable; keep 19c-compatible alternatives for mixed estates.
- The `audit_trail` parameter (incl. `DBMS_FGA.DB + EXTENDED`) is desupported in 23ai — all FGA records go to `UNIFIED_AUDIT_TRAIL`.

## Sources

- [Security Guide 19c — Introduction to Auditing](https://docs.oracle.com/en/database/oracle/oracle-database/19/dbseg/introduction-to-auditing.html)
- [Security Guide 19c — Administering the Audit Trail](https://docs.oracle.com/en/database/oracle/oracle-database/19/dbseg/administering-the-audit-trail.html)
- [Reference 19c — UNIFIED_AUDIT_TRAIL](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/UNIFIED_AUDIT_TRAIL.html)
- [PL/SQL Packages 19c — DBMS_FGA](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_FGA.html)
- [PL/SQL Packages 19c — DBMS_AUDIT_MGMT](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_AUDIT_MGMT.html)
