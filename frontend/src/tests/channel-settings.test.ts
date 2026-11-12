import test from 'node:test';
import assert from 'node:assert/strict';
import { parseChannelSettings,channelSettingsPatch,channelSettingsDigest,verifyChannelOperation,parseChannelResponse } from '../channel-settings.ts';
import type { ChannelConfig,ChannelSettingsView } from '../channel-settings.ts';
const source=():ChannelConfig=>({schema_version:1,channel_ref:'channel:golden',adapter_ref:'text',connector_ref:'connector:golden',scope:'workspace:default',enabled:false,participant_binding:'mapping:guest',credential_ref:'credential:guest'});
const view=():ChannelSettingsView=>({schema_version:1,owner:'owner:test',active_revision:1,pending_restart:false,configured:true,channels:[],sources:[source()],sources_available:true});
test('channel enable binds exact source identity, while unavailable sources remain disableable',()=>{
  const enabled={...source(),enabled:true};assert.equal(channelSettingsPatch(view(),[enabled],'save:channel').channels[0].enabled,true);
  for(const change of [{participant_binding:'owner'},{credential_ref:'secret:other'},{connector_ref:'connector:other'},{adapter_ref:'loopback' as const},{scope:'workspace:other'}])assert.throws(()=>channelSettingsPatch(view(),[{...enabled,...change}],'save:bad'));
  const missing={...view(),sources:[],sources_available:false};assert.throws(()=>channelSettingsPatch(missing,[enabled],'save:bad'));
  assert.equal(channelSettingsPatch(missing,[source()],'save:disable').channels[0].enabled,false);
  for(const bad of [{...view(),sources:[enabled]},{...view(),channels:[source(),source()]},{...view(),channels:[{...source(),token:'leak'}]},{...view(),schema_version:2}])assert.throws(()=>parseChannelSettings(bad));
});
test('channel recovery uses original operation without retransmission and matches Rust digest',async()=>{
  const patch=channelSettingsPatch(view(),[source()],'save:channel-golden');const hash=await channelSettingsDigest(patch);
  assert.equal(hash,'sha256:a26ab8e279015184808ce91750dfb1a90454ab24df97e1772a8ea352d33e7d75');
  const original={schema_version:1,owner:patch.owner,operation_id:patch.operation_id,request_digest:hash,committed:true};verifyChannelOperation(original,patch,hash);
  for(const change of [{owner:'owner:other'},{operation_id:'other'},{request_digest:'wrong'},{committed:false}])assert.throws(()=>verifyChannelOperation({...original,...change},patch,hash));
  assert.throws(()=>parseChannelResponse({schema_version:1,operation_id:patch.operation_id,applied:true,view:view()},patch));
});
