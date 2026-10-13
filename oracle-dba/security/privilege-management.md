# Oracle Privilege Management

## Overview

Privilege management is the foundation of Oracle security and the single highest-impact control in any deployment — overly permissive grants are the root cause of most breaches and insider incidents. Oracle uses discretionary access control with two privilege categories: **system privileges** (a class of operation anywhere, e.g. `SELECT ANY TABLE`) and **object privileges** (an operation on one specific object), grantable directly to users or collected into **roles**.

## Dangerous Privileges to Watch

| Privilege | Risk |
|---|---|
| `DBA` role | Near-total control of the database |
| `SYSDBA` / `SYSOPER` | Administrative OS-bypass access |
| `CREATE/DROP/ALTER/SELECT/EXECUTE ANY ...` | Cross-schema create, destroy, read, execute |
| `GRANT ANY PRIVILEGE` / `GRANT ANY ROLE` | Re-grant to anyone |
| `BECOME USER` | Impersonate any user |
| `EXEMPT ACCESS POLICY` | Bypasses all VPD row-level security |
| `EXEMPT REDACTION POLICY` | Sees redacted data unmasked |

Privilege audit first:

- Call `oracle_dangerous_sys_privs` → lists `ANY`-class, `EXEMPT`-class, `BECOME USER`, `ALTER SYSTEM` holders and password-file SYSDBAs → reconcile each against the authorization matrix.
- Call `oracle_dba_role_members` → direct `DBA` role members → any application or service account here is an immediate finding.
- For one grantee's effective access: call `oracle_user_priv_tree` (param `grantee`, 11.2+) → recursively expands role chains into effective system and object privileges → use this instead of hand-written recursive CTEs.

## Granting and Revoking

> **Operational — preconditions / risk / confirm:** grants take effect immediately; `WITH ADMIN OPTION` / `WITH GRANT OPTION` enable uncontrolled privilege spread — require documented approval. Revoking a system privilege granted WITH ADMIN OPTION does **not** cascade to privileges that grantee re-granted.

```sql
-- System privileges
GRANT CREATE SESSION, CREATE TABLE, CREATE SEQUENCE TO app_user;
GRANT CREATE SESSION TO app_readonly_role;
REVOKE CREATE TABLE FROM app_user;

-- Object privileges (always prefer over ANY privileges)
GRANT SELECT ON hr.employees TO reporting_user;
GRANT UPDATE (salary, job_id) ON hr.employees TO hr_manager;   -- column-level grant
GRANT REFERENCES ON hr.departments TO app_user;                -- needed for FK constraints
GRANT EXECUTE ON hr.process_payroll TO payroll_app;
```

Column-level grants in force — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT grantee, owner, table_name, column_name, privilege
FROM   dba_col_privs
WHERE  owner = 'HR'
ORDER  BY table_name, column_name, grantee;
```

## Roles

> **Operational:** password-protected roles must be enabled per session — confirm applications handle `SET ROLE` before deploying.

```sql
CREATE ROLE app_read_role;
CREATE ROLE app_write_role;
CREATE ROLE sensitive_data_role IDENTIFIED BY "role_password";

GRANT SELECT ON orders.customers TO app_read_role;
GRANT SELECT ON orders.order_lines TO app_read_role;
GRANT app_read_role TO app_write_role;                       -- role hierarchy
GRANT INSERT, UPDATE, DELETE ON orders.customers TO app_write_role;
GRANT app_read_role TO reporting_svc;
GRANT app_read_role TO team_lead WITH ADMIN OPTION;          -- sparingly

-- Session enable / disable
SET ROLE sensitive_data_role IDENTIFIED BY "role_password";
SET ROLE ALL EXCEPT sensitive_data_role;
```

Predefined roles: `CONNECT` is effectively `CREATE SESSION` in 12c+; `RESOURCE` grants object creation — prefer explicit grants for both. Never grant `DBA` to application accounts. Role contents — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT privilege, admin_option FROM dba_sys_privs
WHERE  grantee = 'DBA' ORDER BY privilege;
```

## Least-Privilege Design Pattern

> **Operational:** the schema owner should be locked (`ACCOUNT LOCK`) and never used for connections in production.

```sql
CREATE USER orders_schema IDENTIFIED BY "schema_password" ACCOUNT LOCK;
CREATE USER orders_app    IDENTIFIED BY "app_password" PROFILE app_profile;

GRANT CREATE SESSION TO orders_app;
GRANT SELECT, INSERT, UPDATE ON orders_schema.orders TO orders_app;
GRANT SELECT ON orders_schema.customers TO orders_app;
GRANT EXECUTE ON orders_schema.process_order TO orders_app;

CREATE USER orders_report IDENTIFIED BY "report_password";
GRANT CREATE SESSION TO orders_report;
GRANT SELECT ON orders_schema.orders TO orders_report;
```

## Right-Sizing with DBMS_PRIVILEGE_CAPTURE (12c+)

The definitive tool for right-sizing: capture which privileges an account actually uses, then revoke the rest.

> **Operational — preconditions / risk / confirm:** capture consumes SYSAUX while running. Run in staging, exercise all code paths, then disable and drop the capture.

```sql
BEGIN
  DBMS_PRIVILEGE_CAPTURE.CREATE_CAPTURE(
    name        => 'app_privs_capture',
    description => 'Privileges used by the orders app',
    type        => DBMS_PRIVILEGE_CAPTURE.G_DATABASE);  -- or G_ROLE + roles list
END;
/
EXEC DBMS_PRIVILEGE_CAPTURE.ENABLE_CAPTURE('app_privs_capture');
-- ... run the application workload ...
EXEC DBMS_PRIVILEGE_CAPTURE.DISABLE_CAPTURE('app_privs_capture');
EXEC DBMS_PRIVILEGE_CAPTURE.GENERATE_RESULT('app_privs_capture');
EXEC DBMS_PRIVILEGE_CAPTURE.DROP_CAPTURE('app_privs_capture');
```

Capture results — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
-- Privileges never used = revocation candidates
SELECT username, sys_priv, object_owner, object_name, object_type
FROM   dba_unused_privs
WHERE  capture = 'APP_PRIVS_CAPTURE'
ORDER  BY username;

SELECT username, sys_priv, object_owner, object_name
FROM   dba_used_privs
WHERE  capture = 'APP_PRIVS_CAPTURE'
ORDER  BY username;
```

## PUBLIC Grants

Anything granted to `PUBLIC` is available to every user, present and future — extremely dangerous for sensitive objects and packages.

- Audit: call `oracle_public_dangerous_grants` → `UTL_*`/`DBMS_*` execute rights held by `PUBLIC` → revoke and re-grant narrowly (below). Broader non-package PUBLIC grants — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT owner, table_name, privilege
FROM   dba_tab_privs
WHERE  grantee = 'PUBLIC'
AND    owner NOT IN ('SYS', 'SYSTEM', 'XDB', 'CTXSYS', 'MDSYS', 'ORDSYS')
ORDER  BY owner, table_name;
```

> **Operational — confirm before revoking:** some features (APEX, XDB, monitoring agents) legitimately rely on certain PUBLIC grants; test revocations in staging first.

```sql
REVOKE EXECUTE ON utl_file FROM PUBLIC;
REVOKE EXECUTE ON utl_http FROM PUBLIC;
REVOKE EXECUTE ON utl_tcp  FROM PUBLIC;
REVOKE EXECUTE ON utl_smtp FROM PUBLIC;
GRANT EXECUTE ON sys.utl_file TO etl_process_account;  -- then grant narrowly
```

## Account Hygiene

- Default passwords: call `oracle_users_with_default_password` → any hit is an urgent finding → force a reset immediately.
- Account states: call `oracle_locked_accounts` → review all non-OPEN accounts → confirm schema owners are locked intentionally and no service account is unexpectedly locked or expired.

> **Operational:** profile changes affect logins immediately. Align `FAILED_LOGIN_ATTEMPTS`, `PASSWORD_LIFE_TIME`, and the verify function with your password policy, then confirm with application owners.

```sql
CREATE PROFILE app_profile LIMIT
  FAILED_LOGIN_ATTEMPTS 5
  PASSWORD_LIFE_TIME    90
  PASSWORD_REUSE_TIME   365
  PASSWORD_VERIFY_FUNCTION ora12c_strong_verify_function
  PASSWORD_LOCK_TIME    1/24
  PASSWORD_GRACE_TIME   7;
ALTER USER app_user PROFILE app_profile;
ALTER USER schema_owner ACCOUNT LOCK;
```

Temporary grants should expire — schedule the revoke:

```sql
BEGIN
  DBMS_SCHEDULER.CREATE_JOB(
    job_name   => 'REVOKE_TEMP_ACCESS',
    job_type   => 'PLSQL_BLOCK',
    job_action => 'BEGIN EXECUTE IMMEDIATE ''REVOKE SELECT ON hr.employees FROM contractor_user''; END;',
    start_date => SYSTIMESTAMP + INTERVAL '30' DAY,
    enabled    => TRUE,
    comments   => 'Auto-revoke temporary contractor access');
END;
/
```

## Best Practices

1. **Separate schema owner from application user**; keep the owner locked.
2. **Never grant DBA to application accounts.**
3. **Prefer roles over direct grants** — auditable and revocable.
4. **Avoid `WITH ADMIN`/`GRANT OPTION`** except where documented and approved.
5. **Never grant sensitive objects to PUBLIC.**
6. **Run `DBMS_PRIVILEGE_CAPTURE` before major releases** and right-size production accounts to what is actually used.
7. **Review privilege grants on a schedule** — the tools above make this a five-minute check.
8. **Separation of duties** — the creator should not be the deleter or the exporter.

## Common Mistakes and How to Avoid Them

**`GRANT RESOURCE TO webapp_svc;`** — service accounts should not create objects; grant the specific object privileges needed.

**`SELECT ANY TABLE` instead of specific grants** — grants read on every table in the database; use explicit table grants or a read-only role.

**Forgetting default PUBLIC packages** — packages like `DBMS_BACKUP_RESTORE` and `UTL_FILE` ship granted to PUBLIC; revoke and re-grant narrowly.

**No grant history** — without auditing GRANT/REVOKE you cannot answer "who changed access". Audit it (see `auditing.md`); `oracle_audit_ddl_stream` shows the recent GRANT stream.

**Grants without expiry** — Oracle has no native privilege TTL; use the scheduled-revoke pattern above.

## Compliance Quick Reference

- **SOX:** separation of duties between developers, DBAs, and business users; quarterly privileged-access review; all DBA actions audited.
- **PCI-DSS 7/8:** least privilege for cardholder data; access control lists reviewed at least every six months.
- **HIPAA:** minimum-necessary standard; unique per-individual access (no shared human logins); PHI access logs maintained.

Bypass check: call `oracle_dangerous_sys_privs` → `EXEMPT ACCESS POLICY` bypasses **all** VPD policies — it must be limited to documented ETL/DBA accounts and heavily audited (see `row-level-security.md`).

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance is valid for Oracle Database 19c. `DBMS_PRIVILEGE_CAPTURE` requires 12c+; `oracle_user_priv_tree` works from 11.2+. Re-verify predefined-role contents after upgrades — Oracle keeps trimming legacy roles.

## Sources

- [Security Guide 19c (DBSEG)](https://docs.oracle.com/en/database/oracle/oracle-database/19/dbseg/)
- [SQL Language Reference 19c — GRANT](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/GRANT.html)
- [Reference 19c — DBA_SYS_PRIVS](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/DBA_SYS_PRIVS.html)
- [PL/SQL Packages 19c — DBMS_PRIVILEGE_CAPTURE](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_PRIVILEGE_CAPTURE.html)
