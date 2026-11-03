import type { ProductConfigValidation, ProductOperation } from './types.ts';
import type { ProductConfigView } from './types.ts';
import type { RestoreCandidate } from './data.ts';

export type MaintenanceAction = { kind: 'start_core' | 'stop_core' | 'restart_core' | 'uninstall_plan' }
  | { kind: 'configure_lan'; config_revision: number; validation_digest: string }
  | { kind: 'stage_signed_release'; release_id: string; release_digest: string }
  | { kind: 'upgrade' | 'rollback_compatible_release'; release_id: string; release_digest: string; backup_ref: string }
  | { kind: 'commit_restore'; candidate_ref: string; candidate_digest: string; owner_digest: string; config_revision: number };
export interface MaintenanceCommand {
  schema_version: 1; operation_id: string; expected_revision: number;
  action: MaintenanceAction; requested_deadline: number;
}
export interface MaintenanceSnapshot {
  schema_version: 1; product_revision: number; owner: string;
  host: { schema_version: 1; product_state: string; core_health: string; active_release: string; allowed_operations: string[] };
  operations: ProductOperation[];
}
export const maintenanceComplete = (operation: ProductOperation) => operation.stage === 'committed' && operation.pending_stage === null;
export const maintenanceTerminal = (operation: ProductOperation) => ['committed', 'aborted'].includes(operation.stage) && operation.pending_stage === null;

/** Only advertised, signed-release transactions use this path. Host rechecks
 * release compatibility, backup custody, reauthentication and CAS before IO. */
export function releaseMaintenanceCommand(kind: 'upgrade' | 'rollback_compatible_release', release: string,
  digest: string, backup: string, view: MaintenanceSnapshot, operation: string, now: number): MaintenanceCommand {
  if (view.schema_version !== 1 || view.host.schema_version !== 1 || !view.owner ||
      !view.host.allowed_operations.includes(kind) ||
      !/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,95}$/.test(release) ||
      !/^sha256:[a-f0-9]{64}$/.test(digest) ||
      !/^[a-zA-Z0-9][a-zA-Z0-9:._-]{0,127}$/.test(backup) ||
      !/^[a-zA-Z0-9][a-zA-Z0-9:._-]{0,127}$/.test(operation) ||
      !Number.isSafeInteger(view.product_revision) || view.product_revision < 1 ||
      !Number.isSafeInteger(now) || now < 0 || !Number.isSafeInteger(now + 120_000)) {
    throw new Error('版本、备份或设备状态已变化，请重新读取后再操作。');
  }
  return { schema_version: 1, operation_id: operation, expected_revision: view.product_revision,
    requested_deadline: now + 120_000, action: { kind, release_id: release, release_digest: digest, backup_ref: backup } };
}

export function configureNetworkCommand(config: ProductConfigView, validation: ProductConfigValidation,
  view: MaintenanceSnapshot, operation: string, now: number): MaintenanceCommand {
  if (![1, 2].includes(config.schema_version) || validation.schema_version !== 1 || view.schema_version !== 1 ||
      view.host.schema_version !== 1 || !config.configured || !config.owner || config.owner !== view.owner ||
      config.network_active || validation.revision !== config.active_revision || !validation.valid ||
      !view.host.allowed_operations.includes('configure_lan') ||
      !/^sha256:[a-f0-9]{64}$/.test(validation.network_validation_digest) ||
      !Number.isSafeInteger(config.active_revision) || config.active_revision < 1 ||
      !Number.isSafeInteger(view.product_revision) || view.product_revision < 1 ||
      !Number.isSafeInteger(now) || now < 0 || !Number.isSafeInteger(now + 120_000) ||
      !/^[a-zA-Z0-9][a-zA-Z0-9:._-]{0,127}$/.test(operation)) {
    throw new Error('网络配置或设备状态已变化，请重新读取；当前不能应用 LAN 设置。');
  }
  return { schema_version: 1, operation_id: operation, expected_revision: view.product_revision,
    requested_deadline: now + 120_000, action: { kind: 'configure_lan', config_revision: config.active_revision,
      validation_digest: validation.network_validation_digest } };
}

/** Candidate metadata is read-only server output. The Host still revalidates it. */
export function restoreCommand(candidate: RestoreCandidate, config: ProductConfigView, view: MaintenanceSnapshot,
  operation: string, now: number): MaintenanceCommand {
  if (candidate.schema_version !== 1 || ![1, 2].includes(config.schema_version) || view.schema_version !== 1 ||
      view.host.schema_version !== 1 || !config.configured || !config.owner || config.owner !== view.owner ||
      !Number.isSafeInteger(config.active_revision) || config.active_revision < 1 ||
      !Number.isSafeInteger(view.product_revision) || view.product_revision < 1 ||
      !Number.isSafeInteger(now) || now < 0 || !Number.isSafeInteger(now + 120_000) ||
      !/^[a-zA-Z0-9][a-zA-Z0-9:._-]{0,127}$/.test(operation) ||
      !view.host.allowed_operations.includes('commit_restore') ||
      !/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,95}$/.test(candidate.candidate_ref) ||
      [candidate.candidate_digest, candidate.owner_digest, candidate.store_identity, candidate.execution_fence_digest]
        .some((value) => !/^sha256:[a-f0-9]{64}$/.test(value))) {
    throw new Error('候选或设备状态已变化，请重新读取；当前不能切换主脑。');
  }
  return { schema_version: 1, operation_id: operation, expected_revision: view.product_revision,
    requested_deadline: now + 120_000, action: { kind: 'commit_restore', candidate_ref: candidate.candidate_ref,
      candidate_digest: candidate.candidate_digest, owner_digest: candidate.owner_digest, config_revision: config.active_revision } };
}
