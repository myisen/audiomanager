# Oracle Network Security

## Overview

The network is the attack surface between the Oracle database and every client. Securing it means: connections are **authenticated**, data in transit is **encrypted**, access is **restricted** to authorized hosts, and the listener itself is **hardened** against exploitation. The stack covers the listener (`listener.ora`), Oracle Net (`sqlnet.ora`), TLS/native-encryption wallets, and ACLs for outbound calls (`UTL_HTTP`, `UTL_TCP`, `UTL_SMTP`) made from inside the database.

## Listener Hardening

```ini
# /opt/oracle/network/admin/listener.ora
LISTENER =
  (DESCRIPTION_LIST =
    (DESCRIPTION =
      (ADDRESS = (PROTOCOL = TCPS)(HOST = db-server.corp.example.com)(PORT = 2484))
      (ADDRESS = (PROTOCOL = TCP)(HOST = 10.0.1.50)(PORT = 1521))   # internal IP only
    )
  )

SECURE_REGISTER_LISTENER = (TCPS, IPC)   # block untrusted dynamic registration
ADMIN_RESTRICTIONS_LISTENER = ON         # block remote lsnrctl reconfiguration
# Remove EXTPROC / (PROGRAM = extproc) unless explicitly needed
```

Operational checks from the OS: `lsnrctl status`, `lsnrctl services`, `lsnrctl show dynamic_registration` (should be OFF or show only known services). A listener password is legacy — prefer `ADMIN_RESTRICTIONS_LISTENER = ON`.

## TLS (TCPS) Configuration

> **Operational — preconditions / risk / confirm:** certificate changes affect every client — rotate before expiry (calendar 90 days ahead) and pilot with one client first. Never enable SSLv3, TLS 1.0, or TLS 1.1.

Server certificate into an Oracle wallet:

```bash
orapki wallet create -wallet /opt/oracle/wallet/tls -auto_login
orapki wallet add -wallet /opt/oracle/wallet/tls \
  -dn "CN=db-server.corp.example.com,OU=Database,O=Example Corp,C=US" \
  -keysize 2048 -sign_alg sha256 -pwd WalletP@ss!
orapki wallet export -wallet /opt/oracle/wallet/tls \
  -dn "CN=db-server.corp.example.com,OU=Database,O=Example Corp,C=US" \
  -request /tmp/db-server.csr -pwd WalletP@ss!
# After the CA signs: import the CA cert (-trusted_cert), then the server cert (-user_cert)
orapki wallet display -wallet /opt/oracle/wallet/tls -pwd WalletP@ss!
```

```ini
# sqlnet.ora (server)
SSL_VERSION = 1.2                        # 1.3 if all clients support it
SSL_CLIENT_AUTHENTICATION = FALSE        # TRUE for mutual TLS
SSL_CIPHER_SUITES = (TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
                     TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256)
WALLET_LOCATION =
  (SOURCE = (METHOD = FILE)(METHOD_DATA = (DIRECTORY = /opt/oracle/wallet/tls)))

# listener.ora: TCPS address on port 2484 + the same WALLET_LOCATION
```

Client side — verify the server identity to prevent man-in-the-middle:

```ini
ORCL_TLS =
  (DESCRIPTION =
    (ADDRESS = (PROTOCOL = TCPS)(HOST = db-server.corp.example.com)(PORT = 2484))
    (CONNECT_DATA = (SERVER = DEDICATED)(SERVICE_NAME = ORCL))
    (SECURITY =
      (MY_WALLET_DIRECTORY = /opt/oracle/wallet/client)
      (SSL_SERVER_CERT_DN = "CN=db-server.corp.example.com,OU=Database,O=Example Corp,C=US")))
```

## Native Network Encryption (No Certificates)

Simpler to deploy than TLS, but no server identity verification — still far better than plaintext:

```ini
# sqlnet.ora (both sides)
SQLNET.ENCRYPTION_SERVER = REQUIRED      # REQUIRED, not REQUESTED
SQLNET.ENCRYPTION_CLIENT = REQUIRED
SQLNET.ENCRYPTION_TYPES_SERVER = (AES256, AES192, AES128)
SQLNET.ENCRYPTION_TYPES_CLIENT = (AES256, AES192, AES128)
SQLNET.CRYPTO_CHECKSUM_SERVER = REQUIRED
SQLNET.CRYPTO_CHECKSUM_CLIENT = REQUIRED
SQLNET.CRYPTO_CHECKSUM_TYPES_SERVER = (SHA256)
# Never include 3DES, DES, RC4
```

## IP Allowlisting (Valid Node Checking)

```ini
TCP.VALIDNODE_CHECKING = YES
TCP.INVITED_NODES = (10.0.2.10, 10.0.2.11, 10.0.4.50, 127.0.0.1)
# INVITED_NODES and EXCLUDED_NODES are mutually exclusive; prefer INVITED_NODES
```

Valid node checking is processed by the listener before Oracle authentication — an efficient first line of defense. Keep the listener off the public internet: only the app tier and DBA jump hosts should have routes to 1521/2484 (use a DMZ/app tier, firewalls between tiers, and a separate DBA VLAN).

## Outbound ACLs (UTL_HTTP / UTL_TCP / UTL_SMTP)

Without ACLs these packages enable data exfiltration and internal-network attacks launched from the database host. Since 11g every outbound call requires an explicit ACE.

> **Operational — preconditions / risk / confirm:** ACLs take effect immediately; grant per host + port, never a wildcard to all hosts. Confirm the business need before each grant.

```sql
-- HTTPS to a specific API host
BEGIN
  DBMS_NETWORK_ACL_ADMIN.APPEND_HOST_ACE(
    host => 'api.payment-gateway.com', lower_port => 443, upper_port => 443,
    ace  => xs$ace_type(
      privilege_list => xs$name_list('http'),
      principal_name => 'WEBAPP_SVC', principal_type => xs_acl.ptype_db));
END;
/

-- DNS resolution only
BEGIN
  DBMS_NETWORK_ACL_ADMIN.APPEND_HOST_ACE(
    host => '*',
    ace  => xs$ace_type(
      privilege_list => xs$name_list('resolve'),
      principal_name => 'APP_USER', principal_type => xs_acl.ptype_db));
END;
/

-- Remove an ACE
BEGIN
  DBMS_NETWORK_ACL_ADMIN.REMOVE_HOST_ACE(
    host => 'api.payment-gateway.com', lower_port => 443, upper_port => 443,
    ace  => xs$ace_type(
      privilege_list => xs$name_list('http'),
      principal_name => 'WEBAPP_SVC', principal_type => xs_acl.ptype_db));
END;
/
```

For HTTPS from `UTL_HTTP`, load the outbound wallet first:
`UTL_HTTP.SET_WALLET('file:/opt/oracle/wallet/outbound', 'wallet_password');`

Audit existing ACEs — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT host, lower_port, upper_port, principal, privilege, grant_option
FROM   dba_host_aces
ORDER  BY host, lower_port, principal;
```

## Hardening Checklist

- Dangerous PUBLIC package grants: call `oracle_public_dangerous_grants` → lists `UTL_*`/`DBMS_*` execute rights held by `PUBLIC` → revoke what no application needs, then grant narrowly:

```sql
REVOKE EXECUTE ON utl_http FROM PUBLIC;
REVOKE EXECUTE ON utl_tcp  FROM PUBLIC;
REVOKE EXECUTE ON utl_smtp FROM PUBLIC;
```

- `remote_os_authent` (must be FALSE), `remote_login_passwordfile` (`EXCLUSIVE`/`NONE`), `o7_dictionary_accessibility` (FALSE): call `oracle_nondefault_parameters` → check all three → change via SPFILE with a restart plan if not.
- Privilege review: call `oracle_dangerous_sys_privs` → check password-file SYSDBA holders and `ANY`-class grants → reconcile against the authorization matrix.
- EXTPROC registrations — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT name, network_name FROM v$service WHERE name LIKE 'EXTPROC%';
```

- Session encryption audit — *No dedicated tool — run via `oracle_execute_sql`:* this must return 0 rows in a compliant environment:

```sql
SELECT s.sid, s.serial#, s.username, s.machine, s.program
FROM   v$session s
WHERE  s.username IS NOT NULL
AND    s.sid NOT IN (SELECT c.sid FROM v$session_connect_info c
                     WHERE LOWER(c.network_service_banner) LIKE '%aes%'
                        OR LOWER(c.network_service_banner) LIKE '%encryption%')
ORDER  BY s.username;
```

## Best Practices

1. **TCPS or native encryption `REQUIRED`** — unencrypted Oracle Net exposes credentials, data, and session info to sniffing.
2. **TLS 1.2+ only** — SSL and early TLS are banned by PCI and have known vulnerabilities.
3. **`TCP.VALIDNODE_CHECKING` allowlisting** for app-tier IPs, stopping scans before the authentication layer.
4. **Never expose the listener to the public internet.**
5. **Disable extproc** unless explicitly needed — it allows OS-level code execution from the database.
6. **Grant `UTL_*` packages only to named users, named hosts, named ports.**
7. **`ADMIN_RESTRICTIONS_LISTENER = ON`** — prevents remote reconfiguration (e.g. log-file redirection to overwrite files).
8. **Rotate SSL certificates before expiry** — set reminders 90 days ahead.

## Common Mistakes and How to Avoid Them

**`SQLNET.ENCRYPTION_SERVER = REQUESTED`** — non-compliant clients still connect in the clear; use `REQUIRED` on both sides.

**No `SSL_SERVER_CERT_DN` in tnsnames.ora** — clients accept any valid certificate; MITM becomes possible. Always pin the server DN.

**Wildcard host ACLs** — `host => '*'` with `connect`/`http` lets the grantee call any host; scope ACEs to specific hosts and ports.

## Compliance Quick Reference

- **PCI-DSS 4.1/6.4:** TLS 1.2+ for cardholder data in transit; disable unneeded services and ports.
- **HIPAA 164.312(e):** encryption plus integrity (`CRYPTO_CHECKSUM`) for ePHI in transit.
- **SOX:** `SQLNET.ENCRYPTION_SERVER = REQUIRED` + checksumming satisfies transmission-integrity expectations.

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance is valid for Oracle Database 19c. TLS 1.3 support and cipher defaults vary by release update — test `SSL_VERSION` and cipher suites per release on both server and clients.

## Sources

- [Security Guide 19c (DBSEG)](https://docs.oracle.com/en/database/oracle/oracle-database/19/dbseg/)
- [Net Services Administrator's Guide 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/netag/)
- [Net Services Reference 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/netrf/)
- [PL/SQL Packages 19c — DBMS_NETWORK_ACL_ADMIN](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_NETWORK_ACL_ADMIN.html)
