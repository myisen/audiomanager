# Oracle Deep Data Security

## Overview

Oracle Deep Data Security (Deep Sec) is the Oracle AI Database 26ai framework for database-enforced, application-level authorization: an enterprise application, analytics tool, MCP server, or AI agent gets only the rows, columns, and cells authorized for the requesting end user. Authorization is declarative (`CREATE DATA GRANT`) and driven by an end-user security context, so the database — not application code — enforces policy on every SQL operation, including agent-issued SQL.

Deep Sec is **not** a rename of VPD. VPD uses `DBMS_RLS` policy functions that generate predicates for database users; Deep Sec uses end users, data roles, end-user security contexts, and data grants. Do not mix the two models in a design or migration plan without stating the difference.

## Version and Availability Gate

> Confirm the target is Oracle AI Database 26ai **and** that Deep Sec is available in the selected deployment before proposing any SQL or driver code. Do not present this guide as 19c-compatible.

Release-update capabilities to identify before implementation:

- Base 26ai: end users, data roles, end-user security contexts, data grants, dynamic masking, authorization APIs, mandatory data privileges.
- RU 23.26.2: JDBC Deep Sec APIs and service-provider integration.
- RU 23.26.3: application identity logon, cross-table data grants, local-end-user MFA, OCI IAM database-token authentication.

The Oracle Deep Data Security Guide and the selected client-driver documentation are the authority for version-specific grammar and APIs.

## Choose the Access Model

Identify the requester and connection path first.

| Access pattern | Use when | Required identity boundary |
| --- | --- | --- |
| Direct database access | End user connects with a SQL client via IAM token or local credentials | Database validates the end user and establishes the context |
| Application-mediated access | Application accesses data on a person's behalf | Application identity and end-user identity both validated |
| MCP / agent-mediated access | MCP server or AI agent executes SQL for a person or managed identity | No broad service-account data privileges; establish the end-user context per operation |

Model applications and MCP servers as **application identities** whose database privileges are limited to establishing end-user contexts — never rely on the application's ordinary database user for data authorization.

## Identities, Data Roles, and Contexts

- **End user**: the person or agent subject being authorized; owns no schemas or database objects.
- **Data role**: an authorization role used by data grants; map external/IAM roles to it rather than duplicating app authorization in connection-user grants.
- **Application identity**: the trusted application or MCP server identity that can establish an end-user context.
- **End-user context**: the per-operation or per-session boundary holding the active end user, enabled data roles, and approved attributes.

Context rules: define custom context objects before sending application attributes; limit attributes to values policies actually need (organization, territory, customer ID); specify types and validation. Treat context values as security inputs — derive claims from a validated token, allow only the trusted application identity to set or change the context, use explicit attributes rather than parsing untrusted SQL/JSON in a predicate, and test missing, malformed, and cross-tenant values as deny cases. Never reuse a pooled connection with a prior user's Deep Sec context; follow the driver's documented attach/clear/replace lifecycle.

## Create Data Grants

> **Operational — 26ai only / risk / confirm:** grants change data authorization immediately. Before creating, verify: the target is a supported local table or view (not remote over a DB link); the grant owner can access everything the predicate references; every grantee is a Deep Sec end user or data role (not a conventional user/role); test cases exist for authorized rows, unauthorized rows, unauthorized columns, and DML that would move a row out of scope.

Keep one named, descriptive, stable grant per independently managed policy, and start from the smallest target:

```sql
CREATE DATA GRANT sales_rep_orders
  AS SELECT (order_id, customer_id, order_total)
  ON sales.orders
  WHERE territory_id = ORA_END_USER_CONTEXT.sales.sales_context.territory_id
  TO sales_rep_role;
```

A missing predicate means access to **all** rows of the target — use it only for a consciously broad policy.

## Advanced Authorization

- **Cross-table data grants** (RU 23.26.3+): derive child-row access from granted privileges on related parent rows instead of duplicating parent predicates across every child table. Test both direct and inherited authorization; confirm the administrator has authority over both schemas.
- **Dynamic masking**: use when an unauthorized value must be replaced rather than returned as NULL, based on cell authorization and runtime context. Test that filtering, joins, ordering, exports, and analytics do not leak through masked values or inference. This is Deep Sec semantics — do not assume a `DBMS_REDACT` example behaves the same.
- **Authorization APIs and privilege elevation**: pre-check actions via documented APIs; use elevation only from trusted application code, for a narrow operation, for the shortest scope. Never substitute permanent elevated data roles for end users.
- **Mandatory data privileges**: enforce where object privileges must not bypass Deep Sec; document affected applications and revalidate their connection/administrative privileges after enabling.

## Secure Agent / MCP Flow

1. Authenticate the human or managed agent and the application/MCP server via the supported IAM or local flow.
2. Establish the end-user security context (validated end user, mapped data roles, required attributes only).
3. Execute ordinary parameterized SQL through the driver while the context is active.
4. Let the database enforce data grants — do not add a second, conflicting application-side row filter as the source of truth.
5. Clear or replace the context before returning the connection to the pool.
6. Record application identity, end-user identity, and audit data without logging tokens or raw sensitive values.

A language model, prompt, or MCP tool policy is a caller, not an authorization control; Deep Sec is the data-layer enforcement point.

## Validate and Troubleshoot

Test each policy as the relevant end user, not only as an administrator. Cover: expected rows/columns accessible; unauthorized rows absent; unauthorized columns/cells masked as designed; INSERT/UPDATE cannot create or move values outside authorized scope; users without the data role get nothing; context-attribute changes affect only intended policies; pooled connections retain no prior context.

Troubleshoot in order: (1) end-user provisioning and active data-role mapping, (2) application identity and token validation, (3) active end-user context and attribute values, (4) data-grant target, grantee, privilege, column list, predicate, and time window, (5) mandatory privilege configuration and documented diagnostic tracing. Never resolve an authorization failure by adding `SELECT ANY TABLE`, service-account object grants, or a bypass privilege — fix the missing identity, role, context, or grant.

## Decision Guide

| Requirement | Prefer |
| --- | --- |
| End-user row/column/cell access for apps, analytics, agents | Deep Sec data grants |
| Predicate-based filtering for traditional database users | VPD / `DBMS_RLS` |
| Static presentation-layer value hiding | Data Redaction, when its semantics fit |
| Persistent masking of non-production data | Data Safe or a governed masking process |
| Blocking unapproved SQL statements or connection paths | SQL Firewall, alongside — not instead of — data authorization |

## Sources

- Oracle Deep Data Security Guide: https://docs.oracle.com/en/database/oracle/oracle-database/26/ddscg/
- What Is Oracle Deep Data Security: https://docs.oracle.com/en/database/oracle/oracle-database/26/ddscg/what-is-oracle-deep-data-security.html
- End-User Security Context: https://docs.oracle.com/en/database/oracle/oracle-database/26/ddscg/end-user-security-context.html
- Create Data Grants: https://docs.oracle.com/en/database/oracle/oracle-database/26/ddscg/create-data-grants.html
- Access and Privilege Issues: https://docs.oracle.com/en/database/oracle/oracle-database/26/ddscg/access-and-privilege-issues.html
- Oracle AI Database 26ai New Features: https://docs.oracle.com/en/database/oracle/oracle-database/26/nfcoa/all-nfg.html
