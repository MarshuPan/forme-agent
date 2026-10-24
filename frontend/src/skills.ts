import type { ProductCapabilitySettings, ProductConfigView, ProductSkillConfig, ProductSkillTrust } from './types.ts';

export type SkillDraft = ProductSkillConfig;

export function emptySkill(index = 1): SkillDraft {
  return {
    schema_version: 1, id: `skill:custom-${index}`, summary: '', scope: 'workspace:default',
    version: 1, body: '', enabled: false, trust: 'untrusted',
  };
}

export function capabilitiesFromConfig(config: ProductConfigView | null): ProductCapabilitySettings {
  return config?.capabilities ? structuredClone(config.capabilities) : { schema_version: 1, skills: [] };
}

export function draftFromCapabilities(value: ProductCapabilitySettings | null | undefined): SkillDraft[] {
  return value?.skills ? structuredClone(value.skills) : [];
}

export function capabilitiesFromDraft(skills: SkillDraft[]): ProductCapabilitySettings {
  return { schema_version: 1, skills: structuredClone(skills) };
}

export function skillTrustLabel(trust: ProductSkillTrust): string {
  return trust === 'owner_input' ? '所有者已审阅' : '未信任';
}

export function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).length;
}
