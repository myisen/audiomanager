# Oracle Database Autonomous Recovery Service

## Overview

Oracle Database Autonomous Recovery Service is a managed OCI service for protecting Oracle Databases with centralized backup storage, policy-based retention, immutable backup protection, monitoring, and recovery workflows. Built on Zero Data Loss Recovery Appliance technology, it supports OCI, Oracle Multicloud, and (via Cloud Protect) on-premises databases. For on-premises onboarding with the Fleet Agent and SQLcl `rcv` commands, see `cloud-protect.md`.

## Core Concepts

- **Protected database** — the Recovery Service resource tracking health, data loss exposure, recovery window, protection policy, storage usage, network details, work requests, metrics, and tags.
- **Protection policy** — controls backup retention, retention lock, and (for Multicloud) backup storage location; each protected database has exactly one.
- **Recovery Service subnet** — private IPv4-only subnets in the database VCN used for backup/recovery traffic.
- **Recovery window** — how far back from the current time the database can be recovered.
- **Real-time data protection** — extra-cost continuous redo shipping that drives RPO toward the last sub-second.
- **Long-term retention (LTR) backups** — on-demand backups retained independently (90–3650 days) in Object Storage Infrequent Access.

## Skill 1: Assess Readiness

- Supported releases: Oracle AI Database 26ai RU 23.4+, 21c RU 21.7+ (21.8+ for real-time protection), 19c RU 19.16+ (19.18+ for real-time protection).
- Version and compatibility: call `oracle_instance_overview` → confirm platform and database version; call `oracle_nondefault_parameters` → confirm `compatible >= 19.0.0` → do not onboard below the minimum RU.
- Disable conflicting manual backup scripts first — two operational backup destinations create data-loss scenarios.
- OCI Console limits: Governance & Administration → Tenancy Management → Limits, Quotas and Usage → Autonomous Recovery Service → review protected-database count and recovery-window storage. For Multicloud databases, select the correct subscription before requesting an increase.

## Skill 2: Configure Networking

- Use an IPv4-only subnet (IPv6 unsupported) in the same VCN as the database; recommended size `/24`, `/27` lower bound. Register one Recovery Service subnet per VCN unless adding capacity; multiple protected databases can share it.
- Required stateful ingress to the Recovery Service subnet/NSG: TCP `8005` (backup traffic) and TCP `2484` (RMAN catalog connections + real-time redo). Add matching egress from the database subnet/NSG when traffic is restricted. NSG rules take precedence over security lists for attached VNICs.
- Register in Console: Oracle Database → Database Backups → Recovery Service Subnets → Register Recovery Service subnet (non-sensitive name, compartment, VCN, subnet, up to five NSGs).
- Capacity: when IPs run out, add another subnet to the Recovery Service subnet resource or switch to a different one — always add the new subnet before deleting the old one.

## Skill 3: Choose or Create a Protection Policy

| Policy | Retention |
|---|---:|
| Bronze | 14 days |
| Silver (default) | 35 days |
| Gold | 65 days |
| Platinum | 95 days |

Oracle-defined policies cannot be modified. Custom policies: retention 14–95 days, optional retention lock, optional same-cloud backup storage for Multicloud — the same-cloud choice is **irreversible** once the policy is created.

**Retention lock:** the lock date must be at least 14 days in the future; retention/lock can be changed until it takes effect; after that it cannot be disabled, retention can only increase (max 95 days), backups cannot be modified or deleted before the period ends, and the database cannot move to a different policy.

Create in Console: Oracle Database → Database Backups → Protection Policies → Create protection policy (name, compartment, retention, optional lock date, optional multicloud storage, tags).

## Skill 4: Enable Automatic Backups

Console: database details page → Actions → Configure automatic backups → Enable automatic backups → Backup destination = Autonomous Recovery Service → select protection policy → review backup location and any retention lock → optionally enable real-time data protection → choose termination behavior (retain per policy, or delete after 72 hours) → Save.

Recovery Service then runs Oracle-managed backups: an initial RMAN level 0, successive level 1 incrementals, retained per the selected policy. Do not keep parallel manual operational backup destinations.

## Skill 5: Review Protected Database Health

Console: Oracle Database → Database Backups → Protected Databases → select the database. Review: health, real-time protection, management type, data loss exposure, protection policy, current recovery window, current/projected space usage, last completed/failed backup and duration, DB unique name, version, subscription, backup location, network details, monitoring, work requests, tags.

| Health | Meaning | Typical Action |
|---|---|---|
| `Protected` | Recoverable to any point in the full window; exposure under threshold | Normal monitoring |
| `Warning` | Recoverable in the current window, but exposure above threshold | Check backups, redo shipping, network, metrics, work requests |
| `Alert` | Cannot recover within the window; last backup failed | Urgent: backup failure, subnet, ports, work requests |

Expected data-loss exposure: < 10 seconds with real-time protection, < 120 minutes without. Health and exposure refresh about once per minute.

## Skill 6: Enable Real-Time Data Protection

Supported: 26ai RU 23.4+, 21c RU 21.8+, 19c RU 19.18+. Extra cost. Console: protected database details → linked source database → Actions → Configure automatic backups → enable Real-time data protection → Save. Afterwards verify real-time protection shows enabled and data loss exposure stays within threshold.

## Skill 7: Long-Term Retention Backups

Supported for OCI Databases, Oracle Database@Azure, Oracle Database@Google Cloud. Retention 90–3650 days (1–10 years); stored in Object Storage Infrequent Access, independent of automatic backups. Console: source database details → Backups → Create Backup → Specify long-term backup retention period → set days/years. On database termination, LTR backups follow the chosen termination option (72-hour delete, or retained until their own period ends).

## Skill 8: Recover a Database

Restore options: to latest, to timestamp, or to SCN. Console: database → Actions → Restore → choose the target → confirm. Validate recovery via database state, protected-database health, and the next backup status. LTR backups are restored by creating a new database — in-place restore does not apply.

## Skill 9: Monitor

Metric namespace `oci_recovery_service`; dimensions `resourceId` and `dBUniqueName`. Key metrics: `SpaceUsedForRecoveryWindow` (GB), `ProtectedDatabaseSize` (GB), `ProtectedDatabaseHealth` (0 Protected / 1 Warning / 2 Alert), `DataLossExposure` (seconds). Recommended alarms: health ≥ 1, health = 2, exposure above your RPO threshold, recovery-window storage above your planning threshold. View under protected-database Monitoring, or Observability & Management → Monitoring → Service Metrics.

## Skill 10: APIs, CLI, and IAM

- **Protected database:** `CreateProtectedDatabase` (use `opc-dry-run` first to catch subnet/IP shortages, missing permissions, service/quota limits, duplicate ID, missing or inactive policy), `GetProtectedDatabase`, `UpdateProtectedDatabase`, `FetchProtectedDatabaseConfiguration`, `ChangeProtectedDatabaseCompartment`, `ScheduleProtectedDatabaseDeletion` / `CancelProtectedDatabaseDeletion` / `DeleteProtectedDatabase`.
- **Protection policy / subnet:** standard CRUD + compartment-move APIs (`CreateProtectionPolicy`, `CreateRecoveryServiceSubnet`, ...).
- **LTR backups:** `CreateLongTermBackup`, `ListLongTermBackups`, `GetLongTermBackup`, `UpdateLongTermBackup`, `CancelLongTermBackup`, `DeleteLongTermBackup`.
- **IAM resource types:** `recovery-service-family`, `recovery-service-protected-database`, `recovery-service-policy`, `recovery-service-subnet`, `long-term-backup`, `recovery-service-work-request`. Example:

```text
Allow group RecoveryServiceAdmin to manage recovery-service-family in tenancy
```

## Skill 11: Events and Audit

Event type prefix `com.oraclecloud.autonomousrecoveryservice`, emitted for protected databases, Recovery Service subnets, and protection policies (create, update, delete, change compartment, fetch configuration; most with `.begin`/`.end` variants). Use OCI Audit to review API calls against Recovery Service resources.

## Skill 12: Troubleshoot Backup Failures

**Connection timeouts** — run `tnsping dbrs`; on TNS-12535 check ingress on ports 8005/2484 to the Recovery Service subnet/NSG, egress from the database subnet/NSG, and DNS resolution. Compare `dbrsnames.ora` hostnames with `hosts.txt` from Protected database details → Actions → Download configuration (`dbrsconfig.zip`: dbrsnames.ora, certChainPem, cabundle.txt, hosts.txt — protect these files from unauthorized access).

**Subnet out of IP addresses** — add another subnet to the Recovery Service subnet resource or switch subnets; re-run the failed workflow once capacity is corrected.

**Protected database creation fails** — use the `CreateProtectedDatabase` dry run; review work-request errors for missing IAM policy, missing/inactive protection policy, service limits, compartment quotas, duplicate identity, network reachability.

**Support escalation** — collect work-request errors/logs, console error messages, OCIDs (database, policy, subnet) and timestamps; state clearly the issue relates to Autonomous Recovery Service.

## Operational Checklist (abridged)

1. Release/RU and `compatible >= 19.0.0` confirmed. 2. No conflicting manual backup destination. 3. Limits/quotas and IAM policies verified. 4. IPv4-only subnet sized; ingress + egress on 8005/2484. 5. Protection policy chosen; retention lock decided. 6. Automatic backups enabled; real-time protection decided. 7. Protected database resource created and healthy. 8. Monitoring alarms configured. 9. Restore options and LTR requirements documented.

## Quick Reference

| Item | Value |
|---|---|
| Backup / SQL*Net-RMAN catalog ports | `8005` / `2484` |
| Subnet size | `/24` recommended, `/27` minimum |
| Custom policy retention | 14–95 days |
| Retention lock minimum delay | 14 days |
| LTR backup retention | 90–3650 days |
| Metric namespace | `oci_recovery_service` |
| Health values | 0 Protected, 1 Warning, 2 Alert |
| Exposure thresholds | < 10 s (real-time), < 120 min (without) |

## Sources

- [Using Oracle Database Autonomous Recovery Service](https://docs.oracle.com/en/cloud/paas/recovery-service/dbrsu/using-oracle-database-autonomous-recovery-service.pdf)
- [Database Autonomous Recovery Service](https://docs.oracle.com/en-us/iaas/recovery-service/index.html)
- [About Configuring Protection Policies](https://docs.oracle.com/iaas/recovery-service/doc/overview-protection-policy.html)
- [Backup Retention](https://docs.oracle.com/en-us/iaas/recovery-service/doc/backup-retention.html)
- [Recovery Service Metrics](https://docs.oracle.com/iaas/recovery-service/doc/recovery-service-metrics.html)
- [Managing Protected Databases](https://docs.oracle.com/en-us/iaas/recovery-service/doc/recovery-system.html)
