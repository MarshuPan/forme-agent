import { isRecord, versioned } from './types.ts';
import type { ProductConfigView, ProductSettingFeature } from './types.ts';

export const settingFeatures: ProductSettingFeature[] = ['models', 'skills', 'mcp', 'plugins', 'packages', 'channels', 'proxy', 'network'];

/** Negotiate settings v2 explicitly without weakening other protocol guards. */
export function parseProductSettings(value: unknown): ProductConfigView {
  const invalid = () => new Error('设置协议不兼容，请更新页面或服务后重新读取。');
  if (!isRecord(value) || value.schema_version !== 2 || !isRecord(value.availability)
      || value.availability.schema_version !== 1 || !Array.isArray(value.availability.features)
      || value.availability.features.length !== settingFeatures.length || !Array.isArray(value.models)) throw invalid();
  const remaining = new Set<string>(settingFeatures);
  for (const item of value.availability.features) {
    if (!isRecord(item) || item.schema_version !== 1 || typeof item.feature !== 'string' || !remaining.delete(item.feature)
        || !['configurable', 'unavailable'].includes(String(item.state))
        || (item.state === 'configurable' ? item.reason !== null
          : !['managed_consumer_missing', 'network_paths_missing'].includes(String(item.reason)))) throw invalid();
  }
  for (const item of value.models) versioned(item);
  versioned(value.brain); versioned(value.brain.model_routing); versioned(value.network);
  if (Object.hasOwn(value.network, 'certificate_path') || Object.hasOwn(value.network, 'private_key_path')) throw invalid();
  if (typeof value.network.bind !== 'string' || !Array.isArray(value.network.allowed_hosts)
      || (value.network.firewall !== null && !isRecord(value.network.firewall))) throw invalid();
  if (value.capabilities !== undefined) {
    versioned(value.capabilities);
    if (!Array.isArray(value.capabilities.skills)) throw invalid();
    for (const skill of value.capabilities.skills) versioned(skill);
  }
  const tls = value.network.tls;
  if (tls !== null) {
    versioned(tls);
    if (tls.material_ref !== 'managed:web' || typeof tls.material_configured !== 'boolean'
        || Object.hasOwn(tls, 'certificate_path') || Object.hasOwn(tls, 'private_key_path')) throw invalid();
  }
  if (value.network.firewall !== null) versioned(value.network.firewall);
  return value as unknown as ProductConfigView;
}
