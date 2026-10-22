import type { ProductOperation } from './types.ts';

export type MaintenanceAction = { kind: 'start_core' | 'stop_core' | 'restart_core' | 'uninstall_plan' }
  | { kind: 'stage_signed_release'; release_id: string; release_digest: string };
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
