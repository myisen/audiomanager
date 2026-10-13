# Oracle User Management

## Overview

User management covers creating database users, controlling access through privileges and roles, enforcing policy via profiles, and managing the account lifecycle (locking, expiry, unlocking). In Multitenant (CDB/PDB) environments, **common users** (`C##` prefix, exist across containers) and **local users** (exist in one PDB) both apply. Mismanaged accounts — stale service accounts, default passwords, overprivileged schemas — are among the most common breach vectors.

## Security Screening (tool-driven)

- Call `oracle_locked_accounts` → `LOCKED`/`EXPIRED` accounts and lock timestamps; decide unlock vs. disable.
- Call `oracle_users_with_default_password` → any OPEN account here is an immediate incident.
- Call `oracle_audit_failed_logins` (12.1+) → recurring ORA-1017 bursts suggest lockouts or an attack.
- Call `oracle_dba_role_members` and `oracle_dangerous_sys_privs` → holders of `DBA` and dangerous system privileges; application accounts must not appear.
- Call `oracle_user_priv_tree` (11.2+) → trace effective privileges for a specific user.
- Call `oracle_list_schemas` / `oracle_schema_summary` → inventory of application schemas and their object footprint before dropping or locking.

---

## CREATE USER

**Preconditions:** connect to the correct container (PDB for local users); default/temporary tablespaces exist; profile exists.
**Risk:** created accounts are immediately usable — use `PASSWORD EXPIRE` and/or `ACCOUNT LOCK` when handover is not immediate.

```sql
CREATE USER app_user
  IDENTIFIED BY "S3cur3P@ss!"
  DEFAULT TABLESPACE app_data
  TEMPORARY TABLESPACE temp
  QUOTA 500M ON app_data
  QUOTA UNLIMITED ON app_indexes
  PROFILE app_profile
  ACCOUNT UNLOCK
  PASSWORD EXPIRE;  -- force password change on first login
```

Key clauses: `IDENTIFIED BY` (password) / `EXTERNALLY` (OS) / `GLOBALLY` (LDAP); `DEFAULT TABLESPACE` (avoid SYSTEM); `QUOTA` (without it the user can create objects but allocate no extents); `PROFILE`; `PASSWORD EXPIRE`; `ACCOUNT LOCK`; `NOAUTHENTICATION` (schema-only accounts that must never log in).

### ALTER USER

```sql
ALTER USER app_user IDENTIFIED BY "NewP@ssw0rd!";
ALTER USER app_user QUOTA UNLIMITED ON app_data;
ALTER USER app_user PROFILE high_security_profile;
ALTER USER app_user ACCOUNT LOCK;
ALTER USER app_user ACCOUNT UNLOCK;
ALTER USER app_user ACCOUNT UNLOCK PASSWORD EXPIRE;  -- unlock + force reset
ALTER USER app_user DEFAULT TABLESPACE new_tablespace;
```

### DROP USER

**Risk: irreversible.** Check what the schema owns first with `oracle_schema_summary` (owner). Requires explicit confirmation in production.

```sql
DROP USER app_user;           -- fails if objects exist
DROP USER app_user CASCADE;   -- drops all owned objects — extreme caution
```

---

## Profile Management

A profile is a named set of password and resource limits; every user has exactly one (default: `DEFAULT`). Password limits are always enforced; resource limits only when `RESOURCE_LIMIT=TRUE`.

**Risk:** tightening profiles can lock out service accounts (lockout after `FAILED_LOGIN_ATTEMPTS`) — verify with `oracle_locked_accounts` after rollout.

```sql
CREATE PROFILE app_profile LIMIT
  FAILED_LOGIN_ATTEMPTS    5
  PASSWORD_LOCK_TIME       1/24
  PASSWORD_LIFE_TIME       90
  PASSWORD_REUSE_TIME      365
  PASSWORD_REUSE_MAX       10
  PASSWORD_GRACE_TIME      7
  PASSWORD_VERIFY_FUNCTION ora12c_strong_verify_function
  SESSIONS_PER_USER        10
  CPU_PER_CALL             3000
  CONNECT_TIME             480
  IDLE_TIME                30;

ALTER PROFILE app_profile LIMIT PASSWORD_LIFE_TIME 60 IDLE_TIME 20;
DROP PROFILE app_profile CASCADE;  -- reassigns users to DEFAULT
```

Built-in verify functions: `ora12c_strong_verify_function` (mixed case, digits, specials, ≥9 chars), `ora12c_verify_function`, `verify_function_11G`.

Resource-limit enforcement:

```sql
ALTER SYSTEM SET RESOURCE_LIMIT = TRUE SCOPE=BOTH;
```

Call `oracle_resource_limits` → confirm the parameter is active and check session-level consumption against profile caps.

---

## Tablespace Defaults and Quotas

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT property_name, property_value FROM database_properties
WHERE property_name IN ('DEFAULT_PERMANENT_TABLESPACE','DEFAULT_TEMP_TABLESPACE');
```

```sql
ALTER DATABASE DEFAULT TABLESPACE users;
ALTER DATABASE DEFAULT TEMPORARY TABLESPACE temp;
```

Temporary tablespace groups let Oracle assign users to the least-loaded member:

```sql
CREATE TEMPORARY TABLESPACE temp1
  TEMPFILE '/oradata/temp1_01.tmp' SIZE 2G AUTOEXTEND ON
  TABLESPACE GROUP temp_group;
ALTER USER app_user TEMPORARY TABLESPACE temp_group;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
-- Quota vs. usage per user/tablespace
SELECT username, tablespace_name, bytes/1048576 used_mb,
       max_bytes/1048576 quota_mb
FROM dba_ts_quotas ORDER BY username, tablespace_name;
```

A user with `CREATE TABLE` but no quota can create objects yet fails on first INSERT — always set explicit quota on the default tablespace.

---

## Account Locking and Expiry

Automatic lock happens after `FAILED_LOGIN_ATTEMPTS` consecutive failures, for `PASSWORD_LOCK_TIME` (`UNLIMITED` = manual unlock required). Statuses: `OPEN`, `LOCKED`, `LOCKED (TIMED)`, `EXPIRED`, `EXPIRED (GRACE)`, and combinations.

Diagnose with `oracle_locked_accounts` first; then:

```sql
ALTER USER app_user ACCOUNT UNLOCK;
ALTER USER app_user IDENTIFIED BY "NewP@ssw0rd!";  -- reset expired password
ALTER USER app_user PASSWORD EXPIRE;               -- force change
```

---

## Proxy Authentication

A **proxy user** connects on behalf of a **client user**; sessions appear as the client in `V$SESSION`, preserving end-user identity in audits (connection pools, middle tiers).

```sql
ALTER USER client_user GRANT CONNECT THROUGH proxy_user;
ALTER USER client_user GRANT CONNECT THROUGH proxy_user WITH ROLE reporting_role;
ALTER USER client_user REVOKE CONNECT THROUGH proxy_user;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT proxy, client, role FROM proxy_users;
```

```bash
sqlplus proxy_user[client_user]/<proxy_password>@service_name
```

---

## Common vs. Local Users (CDB/PDB)

Common users (prefix `C##`) exist in CDB$ROOT and optionally all PDBs; local users exist only inside one PDB. **Always create application users as local users inside the application's PDB.**

**Risk:** grants with `CONTAINER=ALL` propagate everywhere — scope deliberately.

```sql
-- Common user (from CDB$ROOT)
ALTER SESSION SET CONTAINER = CDB$ROOT;
CREATE USER c##dba_admin IDENTIFIED BY "P@ssw0rd!" CONTAINER = ALL;
GRANT CREATE SESSION TO c##dba_admin CONTAINER = ALL;

-- Local user (from the target PDB)
ALTER SESSION SET CONTAINER = pdb_production;
CREATE USER app_owner
  IDENTIFIED BY "P@ssw0rd!"
  DEFAULT TABLESPACE app_data
  QUOTA UNLIMITED ON app_data;
GRANT CREATE SESSION, CREATE TABLE, CREATE VIEW, CREATE PROCEDURE, CREATE SEQUENCE TO app_owner;
```

*No dedicated tool — run via `oracle_execute_sql`:*
```sql
SELECT sys_context('USERENV','CON_NAME') AS current_container FROM dual;
```

Removing the `C##` prefix requirement is possible but not recommended:

```sql
ALTER SYSTEM SET "_common_user_prefix" = '' SCOPE=SPFILE;  -- requires restart
```

---

## Best Practices

- Least privilege: application accounts get only the specific privileges needed; never `DBA`/`RESOURCE`. Audit grants with `oracle_dba_role_members`.
- Never use SYS/SYSTEM as an application schema owner.
- Assign a non-DEFAULT profile to every production user.
- Use `PASSWORD EXPIRE` on creation; use `NOAUTHENTICATION` for schema-only accounts.
- Lock unused Oracle-supplied accounts. Generator:
  *No dedicated tool — run via `oracle_execute_sql`:*
  ```sql
  SELECT 'ALTER USER ' || username || ' ACCOUNT LOCK;'
  FROM dba_users
  WHERE oracle_maintained = 'Y'
    AND account_status != 'EXPIRED & LOCKED';
  ```
- Review stale accounts monthly — *No dedicated tool — run via `oracle_execute_sql`:*
  ```sql
  SELECT username, last_login FROM dba_users
  WHERE last_login < SYSDATE - 90 AND account_status = 'OPEN'
  ORDER BY last_login;
  ```
- Use proxy authentication for connection pools to preserve end-user identity.
- Enable Unified Auditing; see `oracle_audit_mode_policies` (12.1+) for current audit settings.

---

## Common Mistakes

**Granting DBA to application users** — detect with `oracle_dba_role_members` / `oracle_dangerous_sys_privs`; remediate by revoking and granting explicit object privileges.

**No quota, then "cannot extend" on first INSERT** — set explicit quota on the default tablespace.

**Leaving DEFAULT profile unhardened** — no lockout, no rotation. Minimal hardening:
```sql
ALTER PROFILE DEFAULT LIMIT
  FAILED_LOGIN_ATTEMPTS 10
  PASSWORD_LIFE_TIME     180;
```

**Editing base tables (USER$) directly** — never; use ALTER USER only.

**DROP USER CASCADE without a pre-check** — irreversible; inspect ownership via `oracle_schema_summary` first and get explicit confirmation.

**Creating local (non-C##) users in CDB$ROOT** — causes confusion and management complexity; create application users inside their PDB.

---

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance valid for Oracle Database 19c unless a newer minimum is explicitly stated.
- Features marked 21c/23c/23ai should be treated as 26ai-capable; keep 19c-compatible alternatives for mixed-version estates.
- In dual-support environments, test syntax and behavior on both 19c and 26ai; defaults and deprecations differ by release update.

## Sources

- [Oracle Database Security Guide 19c — Managing Security for Oracle Database Users](https://docs.oracle.com/en/database/oracle/oracle-database/19/dbseg/managing-security-for-oracle-database-users.html)
- [Oracle 19c SQL Language Reference — CREATE USER](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/CREATE-USER.html)
- [Oracle 19c SQL Language Reference — ALTER USER](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/ALTER-USER.html)
- [Oracle 19c SQL Language Reference — CREATE PROFILE](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/CREATE-PROFILE.html)
- [Oracle 19c Reference — DBA_USERS](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/DBA_USERS.html)
