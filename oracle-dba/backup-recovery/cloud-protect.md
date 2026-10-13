# Oracle Database Zero Data Loss Cloud Protect

## Overview

Oracle Database Zero Data Loss Cloud Protect lets on-premises Oracle Databases use the Zero Data Loss Autonomous Recovery Service in OCI for protected backups, real-time redo protection, and point-in-time recovery. It is operated from the database server through the Cloud Protect Fleet Agent, delivered via SQLcl and invoked with the `rcv` command. For OCI/Multicloud databases see `autonomous-recovery-service.md`; for RMAN fundamentals see `rman-basics.md` and `rman-overview.md`.

## Core Concepts

- **Recovery Service** — the OCI managed service storing protected-database backups and recovery catalog metadata.
- **Cloud Protect Fleet Agent** — SQLcl-based tool on the database server: discovery, onboarding configuration, registration, RMAN integration, protection status.
- **Protected database** — the Recovery Service resource tracking health, recovery window, data loss exposure, policy, metrics, work requests, and tags.
- **Real-time data protection** — optional extra-cost redo shipping to Recovery Service; recommended after onboarding.
- **Protection policy** — backup retention and recovery window; defaults to the Oracle-defined Bronze policy unless the generated configuration is changed before onboarding.
- **SBT library** — `libra.so`, used by RMAN for Recovery Service backup/recovery operations.

## Skill 1: Assess Cloud Protect Readiness

- Host: Linux x86-64. Database: 19c RU 19.18+ or 26ai RU 23.4+, with `COMPATIBLE >= 19.0.0`.
- Version/compatibility: call `oracle_instance_overview` → confirm platform and version; call `oracle_nondefault_parameters` → confirm `compatible >= 19.0.0` → patch up before onboarding if below minimums.
- Encryption: backups to Recovery Service require a TDE wallet set up and **open**, even when TDE is not configured for database data. Call `oracle_tde_wallet_status` (12.1+) → check `wallet_type` and `status` for every con_id (PDBs with local wallets must each be open) → fix any CLOSED wallet before onboarding. Prefer an external key manager (e.g. Oracle Key Vault) for production key custody; a host-local wallet does not protect against host compromise.
- Network: on-premises DNS resolves Recovery Service backup IPs, a DNS listener exists for the on-prem network, the FQDN is registered to the Recovery Service subnet, and firewall rules allow database-to-Recovery-Service traffic.
- SBT library check:

```bash
echo "$ORACLE_HOME"
ls -l "$ORACLE_HOME/lib/libra.so"
```

For 19.27+ and 26ai RU 23.8+, `libra.so` ships under `$ORACLE_HOME/lib`; earlier supported releases need My Oracle Support patch `37855779`.

## Skill 2: Install and Prepare SQLcl

```bash
rpm -ivh /path/to/sqlcl-linux-<version>.rpm     # as root
systemctl enable sql-scheduler && systemctl start sql-scheduler
export PATH=/opt/oracle/sqlcl/bin:$PATH         # for the oracle software owner
export ORACLE_HOME=/u01/app/oracle/product/19.0.0.0/dbhome_1   # required before rcv
/opt/oracle/sqlcl/bin/sql /nolog
```

Persist PATH and ORACLE_HOME per the host standard. In RAC, run the onboarding workflow on the first compute node.

## Skill 3: Configure OCI API Authentication

> **Operational — preconditions / risk / confirm:** add an API signing key for the chosen OCI principal, generate the OCI config file, and store both in a directory readable only by the oracle user. Rotate the key like any other OCI API signing key.

```bash
chmod 700 /secure/oci
chmod 600 /secure/oci/config /secure/oci/oci_api_key.pem
```

```sql
SQL> rcv configure authentication -method api_key -oci_config /secure/oci/config
SQL> rcv show authentication
```

## Skill 4: Discover and Add Databases

> **Operational — preconditions / risk / confirm:** registration creates database credentials and the protected-database resource in OCI. Review the generated configuration with the OCI platform owner before running the add command.

```sql
SQL> rcv add database -auto_discover -generate_config_only -compartment_id <COMPARTMENT_OCID> -recovery_service_subnets <SUBNET_OCID>
```

Cloud Protect generates `add_database.json` (one entry per discovered database). Review every field: `dbUniqueName` and `oracleSid` must match the database; `compartmentId` and `recoveryServiceSubnets` must be the intended OCIDs; `protectionPolicy` defaults to Bronze — change it if needed; `sbtLibrary` must point to the real `libra.so`; `oracleHome` must match the home. Then register:

```sql
SQL> rcv add database -config /u01/app/oracle/rcv/add_database.json
```

Registration validates prerequisites, creates/configures credentials, creates the protected-database resource and waits for it to become active, registers the database with the Recovery Service catalog via RMAN, and configures the database for Cloud Protect. Later operations connect through the generated named connection:

```bash
/opt/oracle/sqlcl/bin/sql -name <DB_UNIQUE_NAME>_rcv_conn
```

## Skill 5: Enable Real-Time Data Protection

> **Operational — database restart required:** real-time redo protection takes effect only after a restart — schedule it.

```sql
SQL> rcv add realtime_redo
SQL> rcv show database    -- verify health after the database is back online
```

Real-time protection is recommended: it lowers expected data-loss exposure when the database is healthy.

## Skill 6: Check Protection Status

```sql
SQL> rcv show database
```

| Health | Meaning | Typical Action |
|---|---|---|
| `Protected` | Full recovery window intact; exposure under threshold | Continue normal monitoring |
| `Warning` | Recoverable within window; exposure above threshold | Check recent backups, redo shipping, agent logs, network, metrics |
| `Alert` | Window cannot be met; latest backup failed | Urgent: backup failures, SBT access, connectivity, work requests |

Expected exposure: < 10 seconds with real-time protection, < 120 minutes without. Cross-check in the OCI Console protected-database details page (health, policy, recovery window, space usage, monitoring charts, work requests).

## Skill 7: Operate with `rcv`

Pattern: `rcv <action> <object> [options]`. Families: `add`, `configure`, `remove`, `show`, `backup`, `run`, `import`. Commands operating directly on the database, RMAN, schedules, real-time redo, backups, checks, or restore ranges require `SYSBACKUP`; object-management commands (policies, subnets, authentication, imports) do not. Use `help rcv` to discover commands; when in doubt connect via the named connection.

```sql
SQL> rcv show database
SQL> rcv show restore_range
SQL> rcv backup database
SQL> rcv run checks
```

## Skill 8: Prepare Restore from Recovery Service

```sql
SQL> rcv show restore_range       -- usable window for the recovery target
SQL> rcv configure rman_env       -- generates RMAN env script + restore template
```

Generated files land under `/u01/app/oracle/rcv/dbs/<DB_UNIQUE_NAME>/rman_env/`. Review and edit `rcv_restore_template.rman`, then run RMAN with the generated environment:

```bash
source /u01/app/oracle/rcv/dbs/<DB_UNIQUE_NAME>/rman_env/rman_env.sh
rman target / catalog /@<DB_UNIQUE_NAME>_DBRS \
  cmdfile /u01/app/oracle/rcv/dbs/<DB_UNIQUE_NAME>/rman_env/rcv_restore_template.rman
```

Example restore template (incomplete recovery opens with `RESETLOGS` after recovery completes):

```sql
RUN {
  SET UNTIL TIME "TO_DATE('2026-06-01 10:00:00','YYYY-MM-DD HH24:MI:SS')";
  RESTORE DATABASE;
  RECOVER DATABASE;
}
ALTER DATABASE OPEN RESETLOGS;
```

> **Operational — risk / confirm:** a restore overwrites the target database. Confirm the recovery target (time/SCN/sequence) with the application owner and get explicit approval before executing.

## Skill 9: Monitor in OCI

On the protected database details page review: protection summary (health, real-time protection, management type — Cloud Protect on-premises databases show Cloud Protect provisioning — data loss exposure, policy, current recovery window), space usage (current/projected recovery-window storage), protected database identity, backup location and compartment, network details, monitoring charts, work requests, and tags. Set OCI Monitoring alarms on key metrics so failures and exposure growth are not discovered only by manual checks.

## Operational Checklist (abridged)

1. Platform, release, and `COMPATIBLE >= 19.0.0` confirmed. 2. TDE wallets open (CDB + local-wallet PDBs). 3. DNS, FQDN, subnet, and firewall ready. 4. `libra.so` present or patch planned. 5. SQLcl installed; `sql-scheduler` running; PATH/ORACLE_HOME set. 6. `rcv configure authentication` done. 7. `add_database.json` generated and reviewed. 8. Registered via `rcv add database -config`. 9. Real-time redo enabled; restart performed. 10. `rcv show database` healthy. 11. OCI alarms configured. 12. RMAN restore environment generated and tested.

## Troubleshooting

- **`rcv` commands fail immediately:** PATH includes `/opt/oracle/sqlcl/bin`; `ORACLE_HOME` set; correct RAC node for the workflow; `help rcv` lists commands.
- **Authentication fails:** OCI config path correct; oracle user can read config and key; OCI user has Recovery Service permissions; key fingerprint matches.
- **Discovery wrong or incomplete:** fix `oracleHome`, `oracleSid`, `dbUniqueName`, `sbtLibrary`, and subnet values in the JSON before `rcv add database -config`.
- **Protected database remains unhealthy:** run `rcv show database`; check agent logs under `/u01/app/oracle/rcv/dbs/<DB_UNIQUE_NAME>/log`; check OCI work requests; check alert log, SBT library accessibility, wallet status, network reachability, and redo-shipping currency.
- **Restore preparation fails:** re-check `rcv show restore_range`; re-run `rcv configure rman_env`; verify `rman_env.sh` points at the expected Oracle home, SBT config, and Recovery Service catalog connection.

## Command Quick Reference

```bash
/opt/oracle/sqlcl/bin/sql /nolog
/opt/oracle/sqlcl/bin/sql -name <DB_UNIQUE_NAME>_rcv_conn
```

```sql
SQL> help rcv
SQL> rcv configure authentication -method api_key -oci_config /secure/oci/config
SQL> rcv add database -auto_discover -generate_config_only -compartment_id <OCID> -recovery_service_subnets <OCID>
SQL> rcv add database -config /u01/app/oracle/rcv/add_database.json
SQL> rcv add realtime_redo
SQL> rcv show database
SQL> rcv show restore_range
SQL> rcv configure rman_env
SQL> rcv run checks
SQL> rcv backup database
```

## Version Notes

- Minimum for the Fleet Agent: Oracle Database 19c RU 19.18 or 26ai RU 23.4.
- `libra.so` is bundled from 19.27 / 26ai RU 23.8; earlier releases need MOS patch 37855779 before onboarding.

## Sources

- [Protecting On-premises Databases using Cloud Protect](https://docs.oracle.com/en-us/iaas/recovery-service/doc/protecting-premises-databases-using-recovery-service.html)
- [Prerequisites for Cloud Protect Fleet Agent](https://docs.oracle.com/en-us/iaas/recovery-service/doc/prerequisites-oracle-cloud-protect-fleet-agent.html)
- [Cloud Protect Fleet Agent Commands in SQLcl](https://docs.oracle.com/en-us/iaas/recovery-service/doc/list-oracle-cloud-protect-fleet-agent-commands-sqlcl.html)
- [Add On-Premises Database to Recovery Service Using Cloud Protect](https://docs.oracle.com/en-us/iaas/recovery-service/doc/add-premises-database-recovery-service-using-cloud-protect.html)
- [Restore On-Premises Database Using Backups from Recovery Service](https://docs.oracle.com/en-us/iaas/recovery-service/doc/restore-premises-database-using-backups-recovery-service.html)
