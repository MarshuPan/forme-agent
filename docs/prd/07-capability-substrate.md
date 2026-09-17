# prd/07 能力基质(Capability Substrate)

模块实施级 PRD。归属 `capabilities` crate。tools/skills/MCP/plugins/hooks 统一索引,产当前 run 的 toolset + 能力证据。MCP/plugin/skill 接入走 `prd/05` 接入框架。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D3**(MCP 一等)、**D4**(Skills)、**D5**(Plugins);结构 **7**(ToolRegistry/ToolsetResolver/CapabilityRegistry)、**8**(MCP)、**9**(skills)、**10**(plugins)。
- **架构**:architecture/03 §1(capabilities crate)、§2.1.1 K 组。**口径**:canonical **§13**(资源统一、least-privilege)、**§15**(信任/内容≠指令)。**依赖 prd/05** 接入框架(McpServer/Plugin/skills 作 `ExternalProvider`)。
- **验收**:主证 **S3**(MCP)、**S4**(skill 按需)、**S5**(plugin 边界)、CapabilityRegistry 矩阵行。
- **不偏离**:MCP/plugin/skill 接入生命周期 = prd/05;资源统一 = canonical §13。

## 1. 职责与边界

- **做**:`CapabilityRegistry`(tools/skills/MCP/plugins/hooks 统一索引)、`ToolRegistry`、`ToolsetResolver`(产 per-run toolset + `CapabilityEvidence`)、`SkillRegistry`(metadata + 正文按需加载)、`McpRegistry`(stdio discovery/allowlist/call/timeout/错误分类/禁用)、`PluginRegistry`(manifest 贡献:tools/skills/MCP/hooks/config)、`HookRegistry`。
- **不做**:执行动作(prd/08)、model call(prd/16)、channel(prd/14);接入生命周期不自造(用 prd/05)。
- **crate**:`capabilities`(依赖 protocol/policy/execution/prd/05)。**M0 范围**:stdio MCP、local skills、local plugin manifest;各类 marketplace / dynamic runtime = M1–M2。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02、prd/04(policy)、prd/05(接入框架)。**不依赖 prd/08**:MCP 工具调用作为 `ActionIntent{backend=Mcp}` 经 harness 咽喉交 execution 执行(见 §5),避免 07↔08 循环。
- **下游被依赖**:prd/03(loop 用 `ToolsetResolver`)、prd/09(context 用 skill metadata / tool schema)、prd/12(coordination 引用能力)。
- **发出事件**:K 组 `CapabilityIndexed`/`ToolsetResolved`/`McpDiscovered`/`McpCallEvent`/`SkillMetadataExposed`/`SkillBodyLoaded`/`PluginContributionRegistered`/`PluginToggled`/`CapabilityEvidenceRecorded`。
- **实现 trait**:`CapabilityRegistry`、`ToolsetResolver`、`SkillRegistry`;`McpServer`/`Plugin` impl prd/05 `ExternalProvider`。
- **必守不变量**:**资源统一**(canonical §13,所有能力单一索引)、least-privilege、**来源不明/风险未知不暴露**、**skill 正文按需**(不无差别塞上下文)、**plugin hook 不绕过执行前重查**、**MCP allowlist**。

## 3. 数据结构与类型

```rust
pub struct CapabilityRegistry { /* 统一索引:tools/skills/mcp/plugins/hooks */ }
pub enum Capability { Tool(ToolRef), Skill(SkillRef), McpTool(McpToolRef), PluginContribution(ContribRef), Hook(HookRef) }
pub struct CapabilityEvidence { pub capability: CapabilityId, pub outcome: Outcome, pub reliability: Reliability }  // 喂 CompetenceGate(prd/13)

pub struct Toolset { pub items: Vec<Capability>, pub scope: Scope }   // 当前 run 可见能力
pub trait ToolsetResolver { fn resolve(&self, ctx: &ResolveContext) -> Result<(Toolset, Vec<CapabilityEvidence>)>; }

pub struct SkillMetadata { pub id: SkillId, pub summary: String, pub scope: Scope, pub version: Version, pub trust: TrustTier }
pub struct SkillBody(String);       // 正文,按需加载

pub struct McpServer { /* impl ExternalProvider(prd/05) */ pub transport: Stdio, pub allowlist: Allowlist }
pub struct McpTool { pub server: ProviderId, pub name: String, pub schema: JsonSchema }
pub enum McpErrorClass { Timeout, SchemaMismatch, ServerError }

pub struct PluginManifest { pub id: PluginId, pub contributions: Vec<PluginContribution>, pub trust: TrustTier }
pub enum PluginContribution { Tool(..), Skill(..), McpServer(..), Hook(..), Config(..) }
pub struct Hook { pub point: HookPoint, pub action: HookAction }   // 不可绕过执行前重查
```

## 4. 接口与签名

```rust
pub trait CapabilityRegistry {
    fn index(&self, source: CapabilitySource) -> Result<()>;      // 纳入统一索引
    fn resolve_toolset(&self, ctx: &ResolveContext) -> Result<Toolset>;  // = prd/05 resolve + policy 过滤
}
pub trait SkillRegistry {
    fn metadata(&self, scope: Scope) -> Vec<SkillMetadata>;       // 默认只这个进上下文
    fn load_body(&self, id: SkillId, trigger: LoadTrigger) -> Result<SkillBody>;  // 选中/被引用才加载 → SkillBodyLoaded
}
// McpRegistry / PluginRegistry 复用 prd/05 ProviderRegistry<McpServer|Plugin> + 专有:
pub trait McpRegistry {
    fn discover(&self, server: ProviderId) -> Result<Vec<McpTool>>;         // metadata + allowlist + server 进程生命周期
    fn prepare_call(&self, tool: McpToolRef, args: Json) -> ActionIntent;   // {backend=Mcp};真正执行在 prd/08
}
```

## 5. 行为/算法/状态机

- **统一索引 → toolset**:`CapabilityRegistry` 汇聚所有来源;`ToolsetResolver::resolve` = prd/05 `resolve` + prd/04 policy 过滤 → 产 `Toolset` + `CapabilityEvidence`,写 `CapabilityIndexed`/`ToolsetResolved`。
- **MCP(canonical §13 / D3)**:stdio discovery → allowlist → tools/resources metadata → server 进程生命周期(经 prd/05)。工具**调用**是 `prepare_call → ActionIntent{backend=Mcp}`,过 harness 咽喉(policy 重查)交 execution(prd/08)`McpBackend` 执行、由其发 `McpCallEvent` 与 timeout/schema/server 错误分类。禁用 server 后其能力不再进 `resolve`。
- **Skill(D4)**:`metadata` 默认可见;仅被选中/被 CoordinationReasoner 引用时 `load_body`(写 `SkillBodyLoaded`);scope/version/trust 可追溯。
- **Plugin(D5)**:manifest 声明贡献 → `PluginContributionRegistered` → enable/disable(`PluginToggled`)→ enabled 贡献进 registry(受 policy);**plugin hook 贡献的工具仍经 `ToolPolicyEvaluated`**(执行前重查,不绕过)。
- **CapabilityEvidence**:从使用结果记可靠性证据,喂 `CompetenceGate`(prd/13)。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`resource_selection_failure`(选错/不明来源)、`execution_failure`(MCP 调用)、`safety_policy_failure`(plugin 试图绕过 re-check)。
- **MCP 错误分类**:`timeout` / `schema_mismatch` / `server_error` → `FailureEvidence.class=tool_failure`。
- **未信任/禁用不暴露**;plugin hook 不能绕过执行前重查;能力来源不明/风险未知不默默暴露给模型。
- **内容≠指令**(canonical §15):MCP 输出、skill 正文作数据、不作命令;标 provenance。
- **能力供应链(canonical §15)**:skills/plugins/MCP 是供应链攻击面(恶意技能、投毒注册表、"被安装即被信任")。未信任能力源默认**最小授权 + 行为完整性校验**,不凭"被安装/被声明"获信任;信任绑定显式、可撤销(接 prd/05 trust binding)。

## 7. 配置与可观测

- **config key**:`mcp.servers.<id>.{command,allowlist,timeout}`、`skills.paths`、`plugins.<id>.{manifest,enabled,trust}`、`hooks.<point>`。
- **可观测**:K 组事件;toolset resolve 可解释(某能力为何在/不在)。

## 8. 实施步骤与顺序

1. `CapabilityRegistry` 统一索引 + `Capability` 模型。
2. `ToolRegistry` + `ToolsetResolver`(resolve = prd/05 + policy)。
3. `McpRegistry`(stdio discovery/allowlist/call/timeout/错误/禁用)impl prd/05。
4. `SkillRegistry`(metadata + 按需 load_body)。
5. `PluginRegistry`(manifest/贡献/toggle)impl prd/05 + `HookRegistry`。
6. `CapabilityEvidence` 记录。

## 9. 测试

- **S3(总纲 §7.6)**:MCP discovery/allowlist/call/timeout/schema/server 错误/禁用后不暴露。
- **S4(总纲 §7.7)**:默认只 metadata;选中才 `SkillBodyLoaded`;未选正文不进上下文。
- **S5(总纲 §7.8)**:manifest 贡献;disabled 不可见;enabled 进 registry 受 policy;plugin hook 经 re-check。
- **CapabilityRegistry 行**:统一索引;能力证据;来源不明/风险未知不暴露。
- **单元**:allowlist 生效;skill 正文仅选中加载;plugin hook 过 re-check;禁用后移出 toolset。

## 10. 验收

- 统一能力索引;per-run toolset + 证据;MCP 一等(discovery/allowlist/call/错误/禁用);skill 按需;plugin 贡献受治理且 hook 不绕过;未知/未信任不暴露;S3/S4/S5 通过。

## 11. 原创性与合规

- capability registry / plugin manifest 是通用模式。**MCP 是公开协议**(Model Context Protocol):**协议兼容允许**,但须记 `source=MCP spec, license, copy_risk, implementation_rule(仅实现协议、不抄 SDK 源码), notice`(canonical §12)。
- **不复制**第三方能力注册表/skill/plugin 的源码、manifest schema、prompt 或命名体系。
