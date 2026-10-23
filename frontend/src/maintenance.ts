import type { ProductOperation } from './types.ts';
import type { ProductConfigView } from './types.ts';
import type { RestoreCandidate } from './data.ts';

export type MaintenanceAction = { kind: 'start_core' | 'stop_core' | 'restart_core' | 'uninstall_plan' }
  | { kind: 'stage_signed_release'; release_id: string; release_digest: string }
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

/** Candidate metadata is read-only server output. The Host still revalidates it. */
export function restoreCommand(candidate: RestoreCandidate, config: ProductConfigView, view: MaintenanceSnapshot,
  operation: string, now: number): MaintenanceCommand {
  if (candidate.schema_version !== 1 || config.schema_version !== 1 || view.schema_version !== 1 ||
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
