# Oracle Database Encryption

## Overview

Encryption protects data at the storage layer: even if an attacker steals datafiles, backup tapes, or exports, the data is unreadable without the keys. It complements access control (prevents unauthorized queries) and auditing (detects unauthorized access). Oracle provides **Transparent Data Encryption (TDE)** — encrypting tablespaces or columns on disk, fully transparent to applications — and **DBMS_CRYPTO**, a PL/SQL API for application-controlled encryption.

TDE requires the **Advanced Security Option** (extra-cost EE option on-premises, including 19c; included in several OCI tiers such as BaseDB EE-HP/EE-EP and ExaDB). Always verify licensing for your version and deployment.

## TDE Key Hierarchy

```
Oracle Wallet / Key Store
  └── Master Encryption Key (MEK)
        └── Tablespace/Column Encryption Keys (DEK)
              └── Encrypted data blocks on disk
```

The MEK lives in the wallet (PKCS#12 file or HSM); DEKs are stored in the database encrypted by the MEK. Data is readable only while the wallet is open.

## Wallet Setup and Management

> **Operational — preconditions / risk / confirm:** EE + Advanced Security license required; `wallet_root` needs a restart. Losing the wallet while datafiles are encrypted means losing the data — back up the wallet to a separate secured location before and after every change. Confirm password handling meets your secrets policy.

```sql
-- 19c+: prefer WALLET_ROOT + TDE_CONFIGURATION
-- (ENCRYPTION_WALLET_LOCATION in sqlnet.ora is deprecated as of 19c)
ALTER SYSTEM SET wallet_root = '/opt/oracle/admin/ORCL/wallet' SCOPE = SPFILE;
ALTER SYSTEM SET tde_configuration = 'KEYSTORE_CONFIGURATION=FILE' SCOPE = BOTH;

-- Create the keystore, open it (all PDBs in a CDB), create + activate the MEK
ADMINISTER KEY MANAGEMENT CREATE KEYSTORE '/opt/oracle/admin/ORCL/wallet'
  IDENTIFIED BY "wallet_password";
ADMINISTER KEY MANAGEMENT SET KEYSTORE OPEN
  IDENTIFIED BY "wallet_password" CONTAINER = ALL;
ADMINISTER KEY MANAGEMENT SET KEY
  IDENTIFIED BY "wallet_password"
  WITH BACKUP USING 'pre_tde_backup' CONTAINER = ALL;
```

Auto-login wallet for unattended restarts:

```sql
ADMINISTER KEY MANAGEMENT CREATE AUTO_LOGIN KEYSTORE
  FROM KEYSTORE '/opt/oracle/admin/ORCL/wallet'
  IDENTIFIED BY "wallet_password";
-- Creates cwallet.sso; LOCAL AUTO_LOGIN binds it to this server only
```

Check keystore state: call `oracle_tde_wallet_status` (12.1+) → review wallet type (`PASSWORD`/`AUTOLOGIN`/`LOCAL_AUTOLOGIN`) and open status per con_id → before any restart, backup, or rotation, confirm status is OPEN everywhere and the type matches the design (auto-login only on physically secured hosts).

Routine wallet operations (require `ADMINISTER KEY MANAGEMENT` privilege):

```sql
ADMINISTER KEY MANAGEMENT SET KEYSTORE OPEN  IDENTIFIED BY "wallet_password";
ADMINISTER KEY MANAGEMENT SET KEYSTORE CLOSE IDENTIFIED BY "wallet_password";
ADMINISTER KEY MANAGEMENT BACKUP KEYSTORE USING 'backup_tag'
  IDENTIFIED BY "wallet_password";
```

## Tablespace Encryption

Tablespace encryption is the recommended TDE deployment — all objects created inside (tables, indexes, LOBs, undo) are encrypted automatically.

> **Operational — preconditions / risk / confirm:** online `ENCRYPT`/`REKEY` is I/O-heavy — schedule off-peak and confirm standby/FRA capacity. Table moves invalidate indexes; rebuild them in the same change window.

```sql
-- New encrypted tablespace (AES256 recommended)
CREATE TABLESPACE sensitive_data
  DATAFILE '/opt/oracle/oradata/ORCL/sensitive_data01.dbf' SIZE 1G AUTOEXTEND ON
  ENCRYPTION USING AES256
  DEFAULT STORAGE (ENCRYPT);

-- Move a table, then rebuild its indexes
ALTER TABLE hr.employees MOVE TABLESPACE sensitive_data;
ALTER INDEX hr.emp_emp_id_pk REBUILD TABLESPACE sensitive_data;

-- Encrypt an existing tablespace online (12c+); offline variant is faster but needs downtime
ALTER TABLESPACE users ENCRYPTION ONLINE ENCRYPT;
```

Online encryption progress — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT tablespace_name, encryption_status
FROM   dba_tablespace_encryption_progress;
```

Coverage audit: call `oracle_encryption_coverage` → shows which tablespaces are encrypted plus user data volume sitting in unencrypted ones → build the move/encrypt backlog from the unencrypted list.

Column-level TDE is more granular but has more overhead; salted columns cannot be used with equality indexes:

```sql
ALTER TABLE hr.employees MODIFY (ssn ENCRYPT USING 'AES256' NO SALT); -- WHERE/equality-join columns
ALTER TABLE patients.records MODIFY (credit_card_number ENCRYPT USING 'AES256'); -- SALT = default
ALTER TABLE hr.employees MODIFY (ssn DECRYPT); -- remove encryption
```

Encrypted columns inventory — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT owner, table_name, column_name, encryption_alg, salt
FROM   dba_encrypted_columns
ORDER  BY owner, table_name;
```

Supported algorithms: `AES256` (recommended; FIPS 140-2; default in 23ai), `AES192`, `AES128` (PCI minimum), `ARIA128/192/256` (Korean standard), `3DES168` (legacy, avoid). `GOST256` is removed in 26ai — do not use for new deployments.

## Key Rotation

> **Operational — preconditions / risk / confirm:** always use `WITH BACKUP` so the old key remains usable; rotate in a maintenance window and verify application health afterwards.

```sql
-- New MEK; Oracle retains the old key for data encrypted with it
ADMINISTER KEY MANAGEMENT SET KEY
  IDENTIFIED BY "wallet_password"
  WITH BACKUP USING 'pre_rotation_backup';

-- Optionally re-encrypt a tablespace DEK under the new MEK (online)
ALTER TABLESPACE sensitive_data ENCRYPTION REKEY;

-- CDB: rotate one PDB or all PDBs
ALTER SESSION SET CONTAINER = pdb_finance;
ADMINISTER KEY MANAGEMENT SET KEY
  IDENTIFIED BY "wallet_password" WITH BACKUP USING 'pdb_rot'
  CONTAINER = CURRENT;
```

Verify the new key is active — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT key_id, creation_time, activation_time, key_use, backed_up
FROM   v$encryption_keys
ORDER  BY creation_time DESC;   -- most recently activated = current MEK
```

## DBMS_CRYPTO (Application-Layer Encryption)

For values the application must encrypt itself (store keys in a key management system, never in the database):

```sql
-- AES256-CBC with PKCS5 padding; store ciphertext and IV together
v_encrypted := DBMS_CRYPTO.ENCRYPT(
  src => UTL_RAW.CAST_TO_RAW(v_plaintext),
  typ => DBMS_CRYPTO.ENCRYPT_AES256 + DBMS_CRYPTO.CHAIN_CBC + DBMS_CRYPTO.PAD_PKCS5,
  key => v_key,   -- RAW(32) generated via DBMS_CRYPTO.RANDOMBYTES(32)
  iv  => v_iv);   -- RAW(16) random per value
v_plaintext := UTL_RAW.CAST_TO_VARCHAR2(
  DBMS_CRYPTO.DECRYPT(src => v_encrypted, typ => ..., key => v_key, iv => v_iv));

-- One-way hash / MAC for integrity checks
v_hash := DBMS_CRYPTO.HASH(src => UTL_RAW.CAST_TO_RAW('value'),
                           typ => DBMS_CRYPTO.HASH_SH256);
```

## Encrypted Backups

TDE-encrypted tablespaces are backed up encrypted by RMAN automatically. For explicit RMAN backup encryption:

> **Operational — risk / confirm:** transparent (keystore) encryption needs Advanced Security and an open keystore; password-based encryption needs no license, but a lost passphrase means an unrestorable backup — store it in the secrets vault.

```sql
RMAN> CONFIGURE ENCRYPTION FOR DATABASE ON;
RMAN> CONFIGURE ENCRYPTION ALGORITHM 'AES256';
RMAN> BACKUP DATABASE;

-- Or passphrase-based, independent of the wallet
RMAN> SET ENCRYPTION ON IDENTIFIED BY "backup_password" ONLY;
RMAN> BACKUP DATABASE;
RMAN> LIST BACKUP SUMMARY;   -- ENCRYPTED column shows YES
```

## Monitoring and Verification

- Coverage: call `oracle_encryption_coverage` → unencrypted tablespaces + user data volume → track the encryption backlog to zero.
- Wallet: call `oracle_tde_wallet_status` → open/type per PDB → alarm on any CLOSED keystore in production.
- TDE key-management activity — *No dedicated tool — run via `oracle_execute_sql`:*

```sql
SELECT event_timestamp, dbusername, action_name, sql_text
FROM   unified_audit_trail
WHERE  action_name LIKE 'ADMINISTER KEY MANAGEMENT%'
ORDER  BY event_timestamp DESC;
```

## Best Practices

1. **Encrypt tablespaces, not just columns** — simpler, less overhead, protects indexes, undo, and temp data too.
2. **Use AES256** for new deployments.
3. **Auto-login wallets only on physically secured hosts** — `cwallet.sso` opens without a password.
4. **Back up the wallet separately from the database** — losing both loses the data.
5. **Always `WITH BACKUP` on key rotation** — preserves the old key for older data.
6. **Audit all `ADMINISTER KEY MANAGEMENT` commands** — unauthorized rotation can make data inaccessible.
7. **Test reads and wallet state after enabling TDE.**
8. **Document and rehearse the wallet-restore runbook** — it is time-critical in DR.

## Common Mistakes and How to Avoid Them

**Wallet next to the datafiles** — put the keystore on a separate filesystem (e.g. `/secure/keystore/ORCL/wallet/`) or use an HSM.

**No wallet backup before rotation** — always tag it: `SET KEY ... WITH BACKUP USING 'before_rotation_<date>';` then verify `backed_up = YES` in `v$encryption_keys`.

**Column encryption on indexed/queried columns** — SALT breaks equality index use; use NO SALT for WHERE-clause columns, or prefer tablespace encryption.

**Exports bypass TDE** — Data Pump decrypts on export; encrypt the dump explicitly:

```bash
expdp system DUMPFILE=full.dmp FULL=Y \
  ENCRYPTION=ALL ENCRYPTION_PASSWORD=<dump_password> ENCRYPTION_ALGORITHM=AES256
```

## Compliance Quick Reference

- **PCI-DSS 3.4/3.5:** strong cryptography for stored PAN (AES-128 minimum, AES-256 recommended); documented key management and rotation.
- **HIPAA 164.312(a)(2)(iv)/(e)(2)(ii):** encrypt ePHI at rest and in transit; AES-256 recommended for healthcare data.
- **GDPR Art. 32:** encryption is a primary "appropriate technical measure"; uncompromised keys may exempt a breach from notification.
- **FIPS 140-2:** Oracle's AES256 is validated; enable `SQLNET.FIPS_140 = TRUE` in sqlnet.ora for FIPS network mode.

## Oracle Version Notes (19c vs 26ai)

- Baseline guidance is valid for 19c; use `WALLET_ROOT` + `TDE_CONFIGURATION` (sqlnet.ora wallet location deprecated since 19c).
- AES256 default since 23ai; `GOST256` deprecated in 23c and removed in 26ai. Keep 19c-compatible alternatives for mixed estates.

## Sources

- [Advanced Security Guide 19c — Using Transparent Data Encryption](https://docs.oracle.com/en/database/oracle/oracle-database/19/asoag/using-transparent-data-encryption.html)
- [Reference 19c — WALLET_ROOT](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/WALLET_ROOT.html)
- [Licensing Information 19c](https://docs.oracle.com/en/database/oracle/oracle-database/19/dblic/Licensing-Information.html)
- [PL/SQL Packages 19c — DBMS_CRYPTO](https://docs.oracle.com/en/database/oracle/oracle-database/19/arpls/DBMS_CRYPTO.html)
- [MOS Doc 2642694.1 — Convert to WALLET_ROOT](https://support.oracle.com/knowledge/Oracle%20Database%20Products/2642694_1.html)
