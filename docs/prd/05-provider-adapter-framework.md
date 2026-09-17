# prd/05 接入框架(Provider/Adapter Framework)

模块实施级 PRD。**跨切契约**——统一"外部能力接入":模型渠道 / 消息渠道 / MCP / 插件本质是同一种可插拔接入。归属 `protocol` 公共 trait + 各消费者 crate(capabilities/communication/models)。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:`prd/01 §9.2` 的四个一等接入缝的**公共契约**——`ModelProvider`(prd/16)、`ChannelAdapter`(prd/14)、`McpServer`/`Plugin`(prd/07)共用的接入生命周期。覆盖 requirements/03 D3/D5/D13/D16 的**接入面**(具体实现在各消费者 PRD)。
- **架构**:architecture/03 §1(crate 图)。**口径**:canonical **§13**(资源统一、least-privilege)、**§15**(来源信任分级)、**§1**(策略在执行点)。
- **验收**:提供 **S3/S5**(MCP/plugin 接入生命周期)、**S12**(渠道接入)所依赖的公共 lifecycle 断言;各 S# 主证在消费者 PRD。
- **不偏离**:接入缝清单 = §9.2;信任分级 = canonical §15;资源最小授予 = canonical §13。

## 1. 职责与边界

- **做**:定义**公共** `ExternalProvider` trait 家族 + `ProviderRegistry` + 接入生命周期(`discovery → config → enable → trust → policy → events`),四个接入缝都据此接入,新渠道/模型/MCP/插件从这里插入、不改核心。
- **不做**:具体实现——MCP/skills/plugins(prd/07)、channel adapter(prd/14)、model provider(prd/16);不执行动作(prd/08);不做 policy 判定(prd/04,接入点**调用** policy)。
- **crate**:`protocol`(公共 trait)+ 各消费者 crate 实现。**M0 范围**:本地注册 + 静态配置;dynamic discovery/远端安装 = M1+。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02(protocol)、prd/04(policy)。
- **下游被依赖**:prd/07(McpRegistry/PluginRegistry/SkillRegistry)、prd/14(ChannelAdapter)、prd/16(ModelProvider)——**它们各自 impl 本框架的公共 trait**,不自造接入流程。
- **发出事件**:公共 lifecycle → 具体事件由消费者按 §2.1.1 发(K 组 `CapabilityIndexed/McpDiscovered/PluginContributionRegistered/PluginToggled/SkillMetadataExposed`、L 组 `CommunicationEventReceived`);本框架定义**何时**发这些的 FSM。
- **实现 trait**:`ExternalProvider`(公共标记 + 接入面)、`ProviderRegistry<P>`。
- **必守不变量**:**least-privilege**(canonical §13,按 AutonomyEnvelope 最小授予)、**来源信任分级**(canonical §15,外部默认 untrusted)、**策略在使用点**(canonical §1,resolve/use 经 prd/04)、**不绕过核心**(provider 不直连 loop/memory,只经 registry + 咽喉)。

## 3. 数据结构与类型

```rust
pub enum ProviderKind { Model, Channel, Mcp, Plugin }

pub trait ExternalProvider {
    fn kind(&self) -> ProviderKind;
    fn id(&self) -> ProviderId;
    fn declared_capabilities(&self) -> CapabilitySet;   // 声明能贡献什么(tools/resources/hooks/…)
    fn trust_default(&self) -> TrustTier;               // 默认 Untrusted(canonical §15)
}

pub enum ProviderLifecycle { Discovered, Configured, Enabled, Trusted(TrustTier), Disabled }

pub struct ProviderRecord<P: ExternalProvider> {
    pub provider: P, pub state: ProviderLifecycle,
    pub config: ProviderConfig,           // credential(via secrets)/manifest/endpoint
    pub grant: Option<AutonomyEnvelope>,  // 最小授予范围
}

pub struct ProviderRegistry<P: ExternalProvider> { /* 索引 + 生命周期 + 事件 */ }
```

四个接入缝各扩展公共面(方法在消费者 PRD 定):`ModelProvider: ExternalProvider`(prd/16)、`ChannelAdapter: ExternalProvider`(prd/14)、`McpServer: ExternalProvider` / `Plugin: ExternalProvider`(prd/07)。

## 4. 接口与签名

```rust
pub trait ProviderRegistry<P: ExternalProvider> {
    fn discover(&self) -> Vec<P>;                                  // 发现(M0:静态配置)
    fn register(&self, p: P, cfg: ProviderConfig) -> Result<ProviderId>;
    fn set_enabled(&self, id: ProviderId, on: bool) -> Result<()>; // enable/disable
    fn bind_trust(&self, id: ProviderId, tier: TrustTier, by: Actor) -> Result<()>;
    fn grant(&self, id: ProviderId, env: AutonomyEnvelope) -> Result<()>; // 最小授予
    fn resolve(&self, ctx: &ResolveContext) -> ActiveSet<P>;       // 当前 run 的可用集(经 policy 过滤)
}
```

## 5. 行为/算法/状态机

- **接入生命周期 FSM**:`Discovered → Configured(credential/manifest) → Enabled → Trusted(binding) → [policy gate] → 可 resolve`;`Disabled` 从 `resolve` 移除。每次转换写对应 K/L 组事件。
- **resolve**:注册表按 `ResolveContext`(session/toolset/envelope)产出当前可用集;**经 prd/04 policy 过滤**(disabled/untrusted/越权不进);产 `CapabilityIndexed`/`ToolsetResolved`(能力类)或活跃 provider/adapter(模型/渠道类)。
- **least-privilege**:provider 授予受 `AutonomyEnvelope` 约束,子代理/workspace 只得最小集(canonical §13)。
- **信任分级**:外部 provider 默认 `Untrusted`;贡献的内容/输出带 provider 的 `trust_tier`,下游按 canonical §15 处理(内容≠指令、未信任低权重)。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`resource_selection_failure`(选错/接入不可信源)、`safety_policy_failure`(越权接入)。
- **disabled/untrusted 不暴露**:不进 `resolve` 结果。
- **内容≠指令**(canonical §15):承载内容的 provider(MCP 输出、渠道输入)其内容只作数据,不作对 forme 的命令;标 provenance。
- **审批**:高信任绑定/高风险 provider 接入可要求 approval(经 prd/04)。

## 7. 配置与可观测

- **config key**:`providers.<kind>.<id>.{enabled,trust,credential_ref,endpoint,manifest}`;credential 经 secrets 分离(prd/16)。
- **可观测**:lifecycle 事件(K/L 组);`resolve` 结果可解释(为何某 provider 在/不在当前可用集)。

## 8. 实施步骤与顺序

1. `ExternalProvider` trait + `ProviderKind` + `ProviderLifecycle`。
2. `ProviderRegistry<P>` 泛型(register/discover/enable/disable/bind_trust/grant/resolve + 事件)。
3. 接入 prd/04 policy 过滤(resolve 经 policy)。
4. 四个接入缝 trait 签名(标记 + 公共面;方法在 07/14/16)。
5. least-privilege + 信任绑定。

## 9. 测试

- **公共 lifecycle**:discover→config→enable→trust→resolve 全链;disable 后从 resolve 移除。
- **未信任/禁用不暴露**:untrusted/disabled provider 不进 `resolve`。
- **least-privilege**:provider 不能越 `grant` 的 envelope。
- (S3/S5/S12 主证在 prd/07/14,引用本框架的公共断言。)

## 10. 验收

- 四个接入缝共用一套接入契约;lifecycle FSM + 事件;disabled/untrusted 不暴露;least-privilege + 信任分级;resolve/use 经 policy。

## 11. 原创性与合规

- provider/registry/adapter(插件式架构)是通用模式,非特定项目专有。
- **不复制**第三方连接器/provider 注册表源码、命名或配置 schema;接入缝抽象源自本项目 `prd/01 §9.2`。
- 具体 provider 引入第三方 SDK 时,在其消费者 PRD 记 source/license/copy_risk/implementation_rule/notice。
