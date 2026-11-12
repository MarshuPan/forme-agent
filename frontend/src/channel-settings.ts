import { isRecord } from './types.ts';
import { mcpDigest, mcpId } from './mcp-settings.ts';
export interface ChannelConfig {
  schema_version:1;channel_ref:string;adapter_ref:'text'|'loopback'|'local_device';connector_ref:string;scope:string;
  enabled:boolean;participant_binding:string;credential_ref:string|null;
}
export interface ChannelSettingsView {schema_version:1;owner:string|null;active_revision:number;pending_restart:boolean;configured:boolean;channels:ChannelConfig[];sources:ChannelConfig[];sources_available:boolean;}
export interface ChannelSettingsPatch {schema_version:1;operation_id:string;expected_revision:number;owner:string;channels:ChannelConfig[];}
const invalid=()=>new Error('渠道设置或原操作回执尚未核实，请重新读取。');
function closed(v:unknown,keys:string[]):asserts v is Record<string,unknown> {
  if(!isRecord(v)||v.schema_version!==1||Object.keys(v).length!==keys.length||keys.some(k=>!Object.hasOwn(v,k)))throw invalid();
}
function entries(v:unknown):ChannelConfig[] {
  if(!Array.isArray(v)||v.length>64)throw invalid();
  for(const c of v){
    closed(c,['schema_version','channel_ref','adapter_ref','connector_ref','scope','enabled','participant_binding','credential_ref']);
    if(![c.channel_ref,c.connector_ref,c.scope,c.participant_binding].every(mcpId)||!['text','loopback','local_device'].includes(String(c.adapter_ref))
      ||typeof c.enabled!=='boolean'||(c.credential_ref!==null&&!mcpId(c.credential_ref)))throw invalid();
  }
  if(new Set(v.map(c=>c.channel_ref)).size!==v.length)throw invalid();
  return v as ChannelConfig[];
}
export function parseChannelSettings(v:unknown):ChannelSettingsView {
  closed(v,['schema_version','owner','active_revision','pending_restart','configured','channels','sources','sources_available']);
  if((v.owner!==null&&!mcpId(v.owner))||!Number.isSafeInteger(v.active_revision)||Number(v.active_revision)<0||Number(v.active_revision)>1_000_000_000
    ||![v.pending_restart,v.configured,v.sources_available].every(b=>typeof b==='boolean'))throw invalid();
  entries(v.channels);if(entries(v.sources).some(c=>c.enabled||c.adapter_ref!=='text'||c.credential_ref===null))throw invalid();
  return v as unknown as ChannelSettingsView;
}
export function channelSettingsPatch(v:ChannelSettingsView,channels:ChannelConfig[],operation:string):ChannelSettingsPatch {
  parseChannelSettings(v);entries(channels);
  if(!mcpId(v.owner)||!mcpId(operation)||!v.configured||channels.some(c=>c.enabled&&(!v.sources_available||!v.sources.some(s=>
    s.channel_ref===c.channel_ref&&s.adapter_ref===c.adapter_ref&&s.connector_ref===c.connector_ref&&s.scope===c.scope&&s.participant_binding===c.participant_binding&&s.credential_ref===c.credential_ref))))throw invalid();
  return {schema_version:1,operation_id:operation,expected_revision:v.active_revision,owner:v.owner,channels:structuredClone(channels)};
}
export function channelSettingsDigest(p:ChannelSettingsPatch){
  return mcpDigest(['forme.product.channel-settings.v1',{schema_version:p.schema_version,operation_id:p.operation_id,expected_revision:p.expected_revision,owner:p.owner,channels:p.channels.map(c=>({schema_version:c.schema_version,channel_ref:c.channel_ref,adapter_ref:c.adapter_ref,connector_ref:c.connector_ref,scope:c.scope,enabled:c.enabled,participant_binding:c.participant_binding,credential_ref:c.credential_ref}))}]);
}
export function verifyChannelOperation(v:unknown,p:ChannelSettingsPatch,digest:string){
  closed(v,['schema_version','owner','operation_id','request_digest','committed']);
  if(v.owner!==p.owner||v.operation_id!==p.operation_id||v.request_digest!==digest||v.committed!==true)throw invalid();
}
export function parseChannelResponse(v:unknown,p:ChannelSettingsPatch):ChannelSettingsView {
  closed(v,['schema_version','operation_id','applied','view']);const view=parseChannelSettings(v.view);
  if(v.operation_id!==p.operation_id||typeof v.applied!=='boolean'||view.owner!==p.owner||view.active_revision<=p.expected_revision)throw invalid();return view;
}
