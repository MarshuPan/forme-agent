import test from 'node:test';
import assert from 'node:assert/strict';
import { computerSettingsDigest, computerSettingsPatch, parseComputerSettings, parseComputerSettingsResponse, verifyComputerSettingsOperation } from '../computer-settings.ts';
const view = () => ({ schema_version: 1, owner: 'owner:settings', active_revision: 7, pending_restart: false, configured: true,
  computer: { schema_version: 1, enabled: false, scope: 'workspace:default', surface_refs: [] },
  available: true, available_scope: 'workspace:default', available_surfaces: ['surface:windows-default'] });
test('desktop save binds exact owner revision and installed placement to Rust golden', async () => {
  const patch = computerSettingsPatch(parseComputerSettings(view()),true,'save:computer-golden');
  assert.equal(await computerSettingsDigest(patch),'sha256:8a414b981e750c75018841f0fa0be00fd8de9308ef1a21fa5541e1c0c71b6072');
  assert.deepEqual(Object.keys(patch),['schema_version','operation_id','expected_revision','owner','computer']);
  for (const altered of [{...view(),schema_version:2}, {...view(),path:'secret'}, {...view(),active_revision:-1},
    {...view(),available:false}, {...view(),available_surfaces:['surface:a','surface:a']}]) {
    assert.throws(() => parseComputerSettings(altered));
  }
});
test('desktop recovery accepts only committed original digest and owner', async () => {
  const patch=computerSettingsPatch(parseComputerSettings(view()),true,'save:computer-golden');
  const digest=await computerSettingsDigest(patch);
  const receipt={schema_version:1,owner:patch.owner,operation_id:patch.operation_id,request_digest:digest,committed:true};
  verifyComputerSettingsOperation(receipt,patch,digest);
  for (const receiptChange of [{owner:'owner:foreign'},{operation_id:'save:other'},{request_digest:'sha256:wrong'},
    {committed:false},{schema_version:2},{ignored:'field'}]) {
    assert.throws(() => verifyComputerSettingsOperation({...receipt,...receiptChange},patch,digest));
  }
  assert.throws(() => parseComputerSettingsResponse({schema_version:1,operation_id:patch.operation_id,applied:true,view:view()},patch));
  const response={schema_version:1,operation_id:patch.operation_id,applied:true,view:{...view(),active_revision:8}};
  assert.equal(parseComputerSettingsResponse(response,patch).active_revision,8);
});
test('unavailable desktop cannot be enabled but can be disabled', () => {
  const unavailable=parseComputerSettings({...view(),available:false,available_scope:null,available_surfaces:[]});
  assert.throws(() => computerSettingsPatch(unavailable,true,'save:no-placement'));
  assert.equal(computerSettingsPatch(unavailable,false,'save:disable').computer.enabled,false);
});
