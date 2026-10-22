import type { ProductConfigView, ProductModelProfilePatch } from './types.ts';
import { ApiError } from './api/client.ts';

export type ModelDraft = ProductModelProfilePatch & { credential: string };

export function emptyModel(): ModelDraft {
  return {
    schema_version: 1, id: 'model:default', provider: 'openai-compatible', model: '', base_url: 'https://api.openai.com/v1',
    credential: '', context_window: 128000, tool_use: true, strength: 'standard',
    input_microunits_per_million: 0, output_microunits_per_million: 0, requests_per_minute: 60,
    tokens_per_minute: 1_000_000, timeout_ms: 120000, fallback_group: null, default: true, tool_schema_versions: [1],
  };
}

export function draftFromConfig(config: ProductConfigView | null): ModelDraft[] {
  if (!config?.models.length) return [emptyModel()];
  return config.models.map(({ credential_configured: _configured, ...model }) => ({ ...model, credential: '' }));
}

/** Read-only server fields and empty credential edits must not enter the patch. */
export function patchFromDraft(models: ModelDraft[]): ProductModelProfilePatch[] {
  return models.map(({ credential, ...model }) => ({ ...model, ...(credential ? { credential } : {}) }));
}

/** These Web errors occur before host dispatch. Other failures stay unknown. */
export function activationRejectedBeforeDispatch(problem: unknown): boolean {
  if (!(problem instanceof ApiError)) return false;
  return new Map([[400, 'InvalidInput'], [401, 'Unauthenticated'], [403, 'CsrfRejected'],
    [409, 'Conflict'], [410, 'Expired'], [429, 'RateLimited']]).get(problem.status) === problem.code;
}
