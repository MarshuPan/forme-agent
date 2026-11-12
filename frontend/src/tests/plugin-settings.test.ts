import test from 'node:test';
import assert from 'node:assert/strict';
import { parsePluginSettings, pluginSettingsPatch, pluginSettingsDigest, verifyPluginOperation, parsePluginResponse } from '../plugin-settings.ts';
import type { PluginConfig, PluginSettingsView } from '../plugin-settings.ts';
const source = (): PluginConfig => ({schema_version:1,plugin_ref:'plugin:golden',source_ref:'source:golden',
  expected_source_digest:`sha256:${'a'.repeat(64)}`,scope:'workspace:default',enabled:false,trust:'untrusted'});
const view = (): PluginSettingsView => ({schema_version:1,owner:'owner:test',active_revision:1,pending_restart:false,
  configured:true,plugins:[],sources:[source()],sources_available:true});
test('plugin edits require the exact published source and preserve disable when it is unavailable', () => {
  const enabled: PluginConfig = {...source(),enabled:true,trust:'owner_input'};
  assert.equal(pluginSettingsPatch(view(),[enabled],'save:plugin').plugins[0].enabled,true);
  for (const change of [{source_ref:'source:foreign'},{plugin_ref:'plugin:foreign'}, {expected_source_digest:`sha256:${'b'.repeat(64)}`},
    {scope:'workspace:foreign'},{trust:'untrusted' as const}]) assert.throws(() => pluginSettingsPatch(view(),[{...enabled,...change}],'save:bad'));
  const missing = {...view(),sources_available:false,sources:[]};
  assert.throws(() => pluginSettingsPatch(missing,[enabled],'save:bad'));
  assert.equal(pluginSettingsPatch(missing,[{...enabled,enabled:false}],'save:disable').plugins[0].enabled,false);
  for (const bad of [{...view(),schema_version:2},{...view(),path:'private'},{...view(),active_revision:-1},
    {...view(),sources:[enabled]},{...view(),plugins:[source(),source()]},
    {...view(),plugins:[{...source(),command:'injected'}]}]) assert.throws(() => parsePluginSettings(bad));
});
test('plugin operation recovery binds the original digest and a later revision is not its receipt', async () => {
  const patch = pluginSettingsPatch(view(),[source()],'save:plugin-golden');
  const digest = await pluginSettingsDigest(patch);
  assert.equal(digest,'sha256:772e8bf2256a51ef35884b18bfab7645750600e1a658a2e7a31513285e732454');
  assert.deepEqual(Object.keys(patch),['schema_version','operation_id','expected_revision','owner','plugins']);
  const receipt = {schema_version:1,owner:patch.owner,operation_id:patch.operation_id,request_digest:digest,committed:true};
  verifyPluginOperation(receipt,patch,digest);
  for (const bad of [{owner:'owner:foreign'},{operation_id:'other'},{request_digest:'wrong'},{committed:false},{path:'private'}]) {
    assert.throws(() => verifyPluginOperation({...receipt,...bad},patch,digest));
  }
  assert.throws(() => parsePluginResponse({schema_version:1,operation_id:patch.operation_id,applied:true,view:view()},patch));
  assert.equal(parsePluginResponse({schema_version:1,operation_id:patch.operation_id,applied:false,view:{...view(),active_revision:3}},patch).plugins.length,0);
});
