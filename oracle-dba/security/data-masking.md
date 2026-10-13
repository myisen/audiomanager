# Oracle Data Masking and Redaction

## Overview

Data masking and redaction protect sensitive data from unauthorized disclosure in complementary ways. **Oracle Data Redaction** (`DBMS_REDACT`) masks data dynamically at query time, in memory — data on disk is unchanged, so indexes, queries, and DML are unaffected. **Oracle Data Safe / permanent masking** transforms data in non-production copies so the original values are gone. Both address the same problem: developers, testers, and analysts should never see real PII/PCI/PHI values.

Data Redaction requires **Enterprise Edition 12c+** (Advanced Security option in 12c, included in EE 19c+); Oracle Data Safe is a cloud service (free tier in OCI). Redaction applies only to users without the `EXEMPT REDACTION POLICY` privilege.

## Redaction Types

| Type | Constant | Behavior |
|---|---|---|
| Full | `DBMS_REDACT.FULL` | Type-appropriate default (0 for numbers, space for strings, epoch for dates) |
| Partial | `DBMS_REDACT.PARTIAL` | Masks a portion; preserves format |
| Regex | `DBMS_REDACT.REGEXP` | `REGEXP_REPLACE`-style masking |
| Random | `DBMS_REDACT.RANDOM` | Random value each query |
| None | `DBMS_REDACT.NONE` | Suspends a policy without dropping it |

> **Operational — preconditions / risk / confirm:** redaction policies take effect immediately for all qualifying sessions. A wrong `expression` can mask data for legitimate users or mask nothing. Test with a non-privileged account before rollout.

## Policy Examples

Full redaction:

```sql
BEGIN
  DBMS_REDACT.ADD_POLICY(
    object_schema => 'HR', object_name => 'EMPLOYEES',
    column_name   => 'SALARY',
    policy_name   => 'REDACT_SALARY_FULL',
    function_type => DBMS_REDACT.FULL,
    expression    => '1=1'   -- always apply; use SYS_CONTEXT for conditional
  );
END;
/
```

Partial redaction — `function_parameters` format is `'input_format, output_format, mask_char, start_pos, end_pos'` (V = variable/data character, F = fixed separator, L = literal keep character):

```sql
-- Credit card: show last 4 only  => ****-****-****-1234
BEGIN
  DBMS_REDACT.ADD_POLICY(
    object_schema => 'PAYMENTS', object_name => 'TRANSACTIONS',
    column_name   => 'CREDIT_CARD_NUMBER',
    policy_name   => 'REDACT_CC_PARTIAL',
    function_type => DBMS_REDACT.PARTIAL,
    function_parameters => 'VVVVFVVVVFVVVVFLLL,VVVV-VVVV-VVVV-LLLL,*,1,12',
    expression    => '1=1'
  );
END;
/

-- SSN: show last 4 => ***-**-1234
BEGIN
  DBMS_REDACT.ADD_POLICY(
    object_schema => 'HR', object_name => 'EMPLOYEES',
    column_name   => 'SSN', policy_name => 'REDACT_SSN',
    function_type => DBMS_REDACT.PARTIAL,
    function_parameters => 'VVVFVVFVVVV,VVV-VV-VVVV,*,1,6',
    expression    => '1=1'
  );
END;
/
```

Regex redaction — for free-text columns:

```sql
BEGIN
  DBMS_REDACT.ADD_POLICY(
    object_schema => 'SUPPORT', object_name => 'TICKETS',
    column_name   => 'DESCRIPTION',
    policy_name   => 'REDACT_EMAIL_IN_TEXT',
    function_type => DBMS_REDACT.REGEXP,
    regexp_pattern        => '[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}',
    regexp_replace_string => '***@***.***',
    regexp_occurrence     => 0,      -- all occurrences
    regexp_match_parameter => 'i',   -- case-insensitive
    expression    => '1=1'
  );
END;
/
```

Conditional redaction — `expression` is evaluated per row/session; when TRUE, redaction applies. This is the mechanism for role-based masking:

```sql
-- Redact salary unless the session holds an HR/PAYROLL context role
BEGIN
  DBMS_REDACT.ADD_POLICY(
    object_schema => 'HR', object_name => 'EMPLOYEES',
    column_name   => 'SALARY',
    policy_name   => 'CONDITIONAL_SALARY_REDACT',
    function_type => DBMS_REDACT.FULL,
    expression    => 'SYS_CONTEXT(''hr_ctx'', ''user_role'') NOT IN (''HR_MGR'', ''PAYROLL'')'
  );
END;
/
```

Random redaction follows the same pattern with `function_type => DBMS_REDACT.RANDOM` (type preserved, value differs on every query).

## Managing Redaction Policies

```sql
-- Add a column to an existing policy
BEGIN
  DBMS_REDACT.ALTER_POLICY(
    object_schema => 'HR', object_name => 'EMPLOYEES',
    policy_name   => 'REDACT_SALARY_FULL',
    action        => DBMS_REDACT.ADD_COLUMN,
    column_name   => 'COMMISSION_PCT',
    function_type => DBMS_REDACT.FULL);
END;
/

-- Change a column's type: action => DBMS_REDACT.MODIFY_COLUMN (+ function_type/parameters)
-- Remove a column:       action => DBMS_REDACT.DROP_COLUMN (+ column_name)

-- Suspend / drop (standalone procedures, NOT ALTER_POLICY action constants)
BEGIN
  DBMS_REDACT.DISABLE_POLICY(object_schema => 'HR',
    object_name => 'EMPLOYEES', policy_name => 'REDACT_SALARY_FULL');
END;
/
BEGIN
  DBMS_REDACT.DROP_POLICY(object_schema => 'HR',
    object_name => 'EMPLOYEES', policy_name => 'REDACT_SALARY_FULL');
END;
/
```

### Reviewing Existing Policies

Call `oracle_redaction_policies` (12.1+) → lists every redaction policy with its expression and enabled flag, plus the protected columns and function types → verify each production policy is intentional and enabled; flag any policy on non-sensitive columns or scoped with `expression => '1=1'` for cleanup.

## Permanent Masking for Non-Production

Oracle Data Safe provides sensitive-data discovery, 50+ masking formats, auditing, and assessments for OCI-hosted databases. Without it, mask permanently during the refresh cycle:

> **Operational — preconditions / risk / confirm:** these UPDATEs are irreversible on the target copy. Run only on non-production databases, inside a tested refresh runbook, with a verified backup of the copy.

```sql
-- Shuffle values (preserves distribution, breaks re-identification)
UPDATE hr.employees e1
SET    salary = (SELECT e2.salary FROM hr.employees e2
                 WHERE ROWNUM = 1 ORDER BY DBMS_RANDOM.VALUE);

-- Consistent hash for emails (same input -> same output)
UPDATE customers
SET    email = LOWER(RAWTOHEX(DBMS_CRYPTO.HASH(
         UTL_RAW.CAST_TO_RAW(LOWER(email)),
         DBMS_CRYPTO.HASH_SH256))) || '@masked.example.com';

-- Randomize dates within a range (preserves relative ordering)
UPDATE patients
SET    date_of_birth = date_of_birth + TRUNC(DBMS_RANDOM.VALUE(-365, 365));

-- Null out the most sensitive fields
UPDATE hr.employees
SET    national_id = NULL, passport_number = NULL;

COMMIT;
```

For a subset-and-mask refresh, use `INSERT ... SELECT` with masking expressions inline (synthetic names, `'555-'` phones, randomized salaries) so non-sensitive columns pass through unchanged.

## Sensitive Column Discovery

Call `oracle_sensitive_columns` (param `owner`) → regex-scans column names for PII/PCI/PHI-style patterns → use the hit list as the masking/redaction backlog and re-run per schema. (Oracle Data Safe automates the same discovery in OCI.)

## Best Practices

1. **Redact at the base table, not views** — views over redacted tables inherit redaction automatically.
2. **Use conditional `SYS_CONTEXT` expressions** to let privileged roles see real data instead of blanket `1=1`.
3. **Redaction is defense-in-depth only** — export tools, database links, and backup access can bypass it; combine with privilege management and auditing.
4. **Never use real production data in non-production** — even with redaction on production; logs and debugging tools leak.
5. **Audit access to redacted columns** (see `auditing.md`).
6. **Document the masking strategy per column** — compliance requires demonstrating what is masked and how.
7. **Test with a non-privileged account** before rollout.

## Common Mistakes and How to Avoid Them

**Assuming export tools respect redaction** — Data Pump, SQL*Plus `COPY`, database links, and GoldenGate all bypass redaction. Check who can see everything: call `oracle_dangerous_sys_privs` → look for `EXEMPT REDACTION POLICY` holders → revoke or document each one. Users with `EXP_FULL_DATABASE`/`DBA` also bypass via Data Pump — protect exports with encryption (see `encryption.md`).

**Inference attacks** — full redaction plus an index still allows binary-search inference of real values; pair redaction with access controls.

**Untested `expression` logic** — an expression that is never TRUE leaks real data to everyone; always test the policy as the intended (non-privileged) user.

## Compliance Quick Reference

- **PCI-DSS 3.3:** mask PAN on display — at most first 6 / last 4 visible. Requirement 3.4 (storage) needs encryption — see `encryption.md`. PCI PAN policy:

```sql
BEGIN
  DBMS_REDACT.ADD_POLICY(
    object_schema => 'PAYMENTS', object_name => 'CARD_DATA',
    column_name   => 'PAN', policy_name => 'PCI_PAN_MASK',
    function_type => DBMS_REDACT.PARTIAL,
    function_parameters => 'VVVVVVFVVVVVVVFVVVV,VVVVVVFVVVVVVVFVVVV,*,7,12',
    expression    => 'SYS_CONTEXT(''USERENV'',''SESSION_USER'') != ''PCI_PROCESSOR'''
  );
END;
/
```

- **HIPAA Safe Harbor:** 18 identifiers removed or masked; redaction covers query-time access; external sharing requires permanent masking.
- **GDPR:** redaction enforces purpose limitation at the data layer; erasure requires permanent deletion/masking, not dynamic redaction.

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance is valid for Oracle Database 19c; `DBMS_REDACT` behavior is stable across 19c–26ai.
- Keep 19c-compatible alternatives for mixed estates; test package behavior per release update.

## Sources

- [PL/SQL Packages 19c — DBMS_REDACT](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_REDACT.html)
- [Advanced Security Guide 19c — Configuring Data Redaction Policies](https://docs.oracle.com/en/database/oracle/oracle-database/19/asoag/configuring-oracle-data-redaction-policies.html)
- [Reference 19c — REDACTION_POLICIES](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/REDACTION_POLICIES.html)
- [Oracle Data Safe Documentation](https://docs.oracle.com/en/cloud/paas/data-safe/index.html)
