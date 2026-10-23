export interface DataPage { schema_version: 1; offset: number; limit: number; snapshot: string | null }
export type DataAction = { Backup: { target: string } }
  | { PrepareRestore: { source: string; target: string } }
  | { PrepareLifecycle: { scope: string; subject: string; operation: 'Retain' | 'Delete' | 'CryptoShred'; expected_version: number } }
  | { Apply: { prepared_by: string; plan_digest: string } };
export interface DataCommand { schema_version: 1; command: string; action: DataAction }
export type DataRead = { Catalog: { page: DataPage } } | { Operations: { page: DataPage } }
  | { Inspect: { command: string } } | { VerifyBackup: { source: string } };
export interface DataSubject { schema_version: 1; scope: string; subject: string; objects: number; deleted_objects: number; projections: string[]; state: string }
export interface DataCatalog { schema_version: 1; kind: 'Catalog'; version: number; pending_cleanup: boolean; digest: string; total: number; next_offset: number | null; subjects: DataSubject[] }
export interface DataPlan { schema_version: 1; digest: string; expires_at: number; operation: string; scope: string | null; subject: string | null; objects: number; source: string | null; target: string | null; irreversible: boolean }
export interface RestoreCandidate {
  schema_version: 1; candidate_ref: string; candidate_digest: string;
  owner_digest: string; store_identity: string; execution_fence_digest: string;
}
export interface DataBackup {
  schema_version: 1; object: string; digest: string; events: number; created_at: number; verified: boolean;
  restoration?: RestoreCandidate;
}
export interface DataReceipt {
  schema_version: 1; command: string; binding: string; action: string; prepared_by: string | null; state: 'Prepared' | 'Confirmed' | 'Unknown';
  plan: DataPlan | null; backup: DataBackup | null;
  lifecycle: { schema_version: 1; scope: string; subject: string; operation: string; cleaned_projections: number; destroyed_keys: number; remote: string } | null;
}
export interface DataOperations { schema_version: 1; kind: 'Operations'; digest: string; total: number; next_offset: number | null; operations: DataReceipt[] }
export type DataView = DataCatalog | DataOperations | { schema_version: 1; kind: 'Receipt'; receipt: DataReceipt | null } | ({ kind: 'Verification' } & DataBackup);
export const dataPage = (offset = 0, snapshot: string | null = null): DataPage => ({ schema_version: 1, offset, limit: 50, snapshot });
export const dataState = { Prepared: '等待确认计划', Confirmed: '操作已确认', Unknown: '结果待核实' };
