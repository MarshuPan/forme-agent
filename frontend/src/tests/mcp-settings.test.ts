import test from 'node:test';
import assert from 'node:assert/strict';
import { mcpSettingsDigest, mcpSettingsPatch, parseMcpSettings, parseMcpSettingsResponse, verifyMcpSettingsOperation,
  parseMcpPrograms, mcpProgramCommand, mcpProgramDigest, verifyMcpProgramOperation, mcpDiscoveryRequest, mcpDigest } from '../mcp-settings.ts';
import type { McpServer, McpSettingsView, McpProgramCatalog } from '../mcp-settings.ts';
const server = (): McpServer => ({schema_version:1,server_ref:'mcp:golden',program_ref:`mcp-program:${'a'.repeat(64)}`,scope:'workspace:default',
  allowlist:{schema_version:1,tools:['fixture.read'],resources:[]},enabled:false,trust:'untrusted',timeout:1000,output_budget:8192});
const view = (): McpSettingsView => ({schema_version:1,owner:'owner:test',active_revision:1,pending_restart:false,configured:true,mcp_servers:[],available:true});
const catalog = (): McpProgramCatalog => ({schema_version:1,installation_ref:'install:fixture',owner_ref:'owner:test',authority_epoch:1,revision:2,
  choices:[{schema_version:1,source_ref:'source:fixture',source_digest:`sha256:${'b'.repeat(64)}`,scope:'workspace:default',sandbox_profile_ref:'windows-appcontainer-mcp-stdio-v1'}],
  programs:[{schema_version:1,program_ref:server().program_ref,registration_digest:`sha256:${'a'.repeat(64)}`,source_ref:'source:fixture',scope:'workspace:default',sandbox_profile_ref:'windows-appcontainer-mcp-stdio-v1',revoked:false}]});

test('MCP list edit matches Rust golden and never carries other managed secrets', async () => {
  const patch = mcpSettingsPatch(parseMcpSettings(view()),[server()],'save:mcp-golden');
  assert.equal(await mcpSettingsDigest(patch),'sha256:8bd56b2367c68b5af6824154be255750344591b44cf9b94f3a2353e49fe1792f');
  assert.deepEqual(Object.keys(patch),['schema_version','operation_id','expected_revision','owner','mcp_servers']);
  for (const changed of [{...view(),schema_version:2},{...view(),path:'secret'},{...view(),active_revision:-1},
    {...view(),mcp_servers:[server(),server()]},{...view(),mcp_servers:[{...server(),args:['unregistered']}]},
    {...view(),mcp_servers:[{...server(),allowlist:{schema_version:1,tools:['*'],resources:[]}}]}]) assert.throws(() => parseMcpSettings(changed));
  assert.throws(() => mcpSettingsPatch({...view(),available:false},[{...server(),enabled:true,trust:'owner_input'}],'save:bad'));
  assert.equal(mcpSettingsPatch({...view(),available:false},[server()],'save:disable').mcp_servers[0].enabled,false);
});
test('MCP save recovery requires original digest and does not mistake current configuration for receipt', async () => {
  const patch = mcpSettingsPatch(view(),[server()],'save:mcp-golden'), hash = await mcpSettingsDigest(patch);
  const original = {schema_version:1,owner:patch.owner,operation_id:patch.operation_id,request_digest:hash,committed:true};
  verifyMcpSettingsOperation(original,patch,hash);
  for (const change of [{owner:'owner:foreign'},{request_digest:'sha256:wrong'},{operation_id:'other'},{committed:false},{extra:'field'}]) {
    assert.throws(() => verifyMcpSettingsOperation({...original,...change},patch,hash));
  }
  assert.throws(() => parseMcpSettingsResponse({schema_version:1,operation_id:patch.operation_id,applied:true,view:view()},patch));
  assert.equal(parseMcpSettingsResponse({schema_version:1,operation_id:patch.operation_id,applied:false,view:{...view(),active_revision:3}},patch).mcp_servers.length,0);
});
test('program management binds fixed source and rejects altered operation receipts', async () => {
  const c = parseMcpPrograms(catalog());
  const command = mcpProgramCommand(c,{kind:'register',source_ref:c.choices[0].source_ref,source_digest:c.choices[0].source_digest},'register:test',1000);
  const hash = await mcpProgramDigest(command);
  const receipt = {schema_version:1,operation_id:command.operation_id,command_digest:hash,revision:3,program:c.programs[0]};
  verifyMcpProgramOperation(receipt,command,hash);
  assert.throws(() => mcpProgramCommand(c,{kind:'register',source_ref:'source:foreign',source_digest:c.choices[0].source_digest},'register:test',1000));
  assert.throws(() => parseMcpPrograms({...c,programs:[{...c.programs[0],program_ref:'program:wrong'}]}));
  for (const change of [{command_digest:'wrong'},{operation_id:'other'},{revision:2},{program:{...c.programs[0],revoked:true}}]) {
    assert.throws(() => verifyMcpProgramOperation({...receipt,...change},command,hash));
  }
});
test('discovery uses saved parameters and a deterministic original run before submission', async () => {
  const s = {...server(),enabled:true,trust:'owner_input' as const}, p = catalog().programs[0];
  const original = await mcpDiscoveryRequest('owner:test',s,p,'discovery:first');
  assert.equal(original.run,`run:owner-mcp:${await mcpDigest(['owner:test',original.request.session,'discovery:first'])}`);
  assert.equal(original.request.spec.program_ref,p.program_ref);
  assert.equal(original.request.spec.output_budget_bytes,s.output_budget);
  assert.notEqual(original.run,(await mcpDiscoveryRequest('owner:test',s,p,'discovery:second')).run);
  await assert.rejects(mcpDiscoveryRequest('owner:test',s,{...p,revoked:true},'discovery:bad'));
  await assert.rejects(mcpDiscoveryRequest('owner:test',{...s,scope:'workspace:other'},p,'discovery:bad'));
});
