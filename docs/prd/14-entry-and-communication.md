# prd/14 入口 + 沟通(Entry & Communication Fabric)

模块实施级 PRD。归属 `cli` + `gateway` + `communication` crate。CLI 与 Local Gateway 双入口(同 run/同 event);Communication Fabric 是对外触手的统一控制面。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D1**(CLI + Local Gateway 双入口)、**D16**(Communication Fabric);结构 **4**(双入口)、**21**(Communication Fabric 数据边界)。
- **架构**:architecture/03 §1(communication/gateway/cli)、§2.6(沟通对象);设计底稿 archive/planning/12(统一网关与沟通触手)。
- **口径**:canonical **§10**(身份:owner / external counterparty / agent representing owner;代表≠伪装)、**§13**(内部编排 ≠ 对外 A2A,走不同通道)。**依赖 prd/05**(ChannelAdapter)。
- **验收**:主证 **S1**(双入口同协议)、**S12–S17**(沟通簇)。
- **不偏离**:入口不拥有 loop(prd/03);adapter 先进 Gateway、不直调 harness;对外会话有目的/范围/期限/终止。

## 1. 职责与边界

- **做**:`CLI`(人机入口)、`Local Gateway/App Server`(控制协议 + 未来 UI/API/后台入口)、`Communication Fabric`(`CommunicationEvent`/`CommunicationSession`、`ChannelAdapter`(经 prd/05)、对外膜:`DisclosurePolicy`/`TerminationPolicy`/`ExternalCommunicationGrant`、`ParticipantProfile`、代表身份)。
- **不做**:loop(prd/03,入口只提交 run/消费 event)、policy 判定(prd/04)、harness(prd/03)。
- **crate**:`cli`、`gateway`、`communication`。**M0 范围**:**text + CLI/Local Gateway 一个 adapter**;真实多平台 adapter/语音/视频/A2A 协议 = M2(M0 只建**数据模型 + enforce 点**,用模拟事件验证)。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02、prd/03(harness——`submit_run`)、prd/04(policy——审批 + 披露)、prd/05(接入框架——`ChannelAdapter`)。
- **下游被依赖**:无(栈顶入口)。
- **发出事件**:L 组 `CommunicationEventReceived`/`CommunicationSessionOpened`/`CommunicationSessionTerminated`/`ExternalCommunicationGranted`/`DisclosurePolicyApplied`/`CommunicationProposalEmitted`;触发 A 组 `RunAccepted{source=communication}`(经 Gateway)。
- **实现**:`ChannelAdapter` impl prd/05 `ExternalProvider`。
- **必守不变量**:**入口不拥有 loop**(S1)、**adapter 不绕过 Gateway 直调 loop/写 memory**(S12)、**外部会话有范围/期限/终止**(S13)、**披露阻断敏感**(S14)、**代表≠伪装 owner**(canonical §10,S15)、**本地设备需授权**(S16)、**A2A bounded 短会话强终止**(S17)、**审批 Gateway-compatible**(D12)。

## 3. 数据结构与类型

```rust
pub struct CommunicationEvent { pub modality: Modality, pub carrier: Carrier, pub channel_adapter: ProviderId,
    pub participant: ParticipantId, pub scope: Scope, pub session_ref: Option<SessionId> }
pub enum Modality { Text, Voice, Image, Video }   // M0 只跑 Text
pub enum Carrier { Software, Hardware }

pub struct CommunicationSession { pub participant: ParticipantProfile, pub purpose: CommunicationPurpose,
    pub modality_set: Vec<Modality>, pub disclosure_policy: DisclosurePolicy, pub termination_policy: TerminationPolicy,
    pub autonomy_envelope: AutonomyEnvelope, pub ttl: Timebox, pub budget: Budget, pub transcript_policy: TranscriptPolicy }

pub enum ParticipantProfile { Owner, External(ExternalId), AgentRepresentingOwner }   // canonical §10
pub struct DisclosurePolicy { pub allow: Vec<DisclosureRule>, pub representation: RepresentationRule }  // 代表边界折叠于此(canonical §10)
pub struct ExternalCommunicationGrant { pub purpose: CommunicationPurpose, pub disclosure: DisclosurePolicy,
    pub ttl: Timebox, pub budget: Budget, pub transcript_policy: TranscriptPolicy }
pub struct LocalDeviceChannelAdapter { pub device_grant: DeviceGrant, pub observation_scope: ObservationScope, pub retention: RetentionPolicy }  // M0 数据模型
```

## 4. 接口与签名

```rust
pub trait ChannelAdapter {                                  // impl prd/05 ExternalProvider
    fn normalize(&self, raw: RawInput) -> Result<CommunicationEvent>;   // → 标准事件
}
pub trait Gateway {
    fn submit(&self, ev: CommunicationEvent) -> Result<RunId>;          // → RunRequest → harness
    fn stream_events(&self, run: RunId) -> EventStream;
    fn approve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> Result<()>;  // Gateway-compatible
}
pub trait CommunicationFabric {
    fn open_session(&self, grant: ExternalCommunicationGrant) -> Result<SessionId>;
    fn apply_disclosure(&self, req: DisclosureRequest) -> DisclosureOutcome;   // refuse/blur/approve/answer
    fn terminate(&self, session: SessionId, reason: TerminationReason) -> Result<()>;
}
```

## 5. 行为/算法/状态机

- **入口(S1/S12)**:raw → `ChannelAdapter::normalize` → `CommunicationEvent` → `Gateway::submit` → `RunRequest`/`CommunicationSession` → harness。CLI 与 Gateway **同协议**;**adapter 不能绕过 Gateway**。
- **外部会话(S13)**:`ExternalCommunicationGrant`(purpose/disclosure/ttl/budget/transcript)→ `CommunicationSessionOpened` → 对话 → 超 TTL/轮次/预算/目的偏离 → `CommunicationSessionTerminated` 或要求确认。M0 模拟外部参与者。transcript 进 candidate/evidence、不自动写 stable。
- **披露(S14)**:外部请求超 policy → `DisclosurePolicyApplied`(refuse/blur/approve/answer)→ audit;低置信候选不对外作确定事实。
- **代表身份(S15)**:`DisclosurePolicy.representation` → 代表 owner 表达但**不伪装**;高影响/未确认 → approval;误差进 FailureEvidence/candidate。
- **本地设备(S16)**:`DeviceGrant`/scope/expiry;未授权 → 不产 `CommunicationEventReceived`;撤销后拒。
- **A2A bounded(S17)**:强目的/强终止;越界请求拒/转审批/终止;digest 进 candidate。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`safety_policy_failure`(adapter 绕过)、`trust_failure`。
- **adapter 不绕过 Gateway/policy/audit**。
- **provenance 赋值权(canonical §15)**:`Provenance.trust_tier` 由 **Gateway/harness(受信代码)**按入口与认证盖章,**绝不**由 adapter 或内容自报——防被攻破的 adapter 把外部内容伪标为 owner 输入绕过防投毒。
- **对外膜**:外部会话有界;披露阻断;代表身份;transcript → candidate/evidence 不自动 stable。
- **审批 Gateway-compatible**(D12):CLI/Gateway 共享审批事件。

## 7. 配置与可观测

- **config key**:`channels.<id>.{adapter,enabled}`、`disclosure.policies`、`external_grants.*`、`device_grants.*`。
- **可观测**:L 组事件;披露/终止/代表可解释、进 audit。

## 8. 实施步骤与顺序

1. `CommunicationEvent`/`CommunicationSession` 模型 + `ParticipantProfile`。
2. CLI + Local Gateway 双入口(同 `RunRequest`/同 event)。
3. `ChannelAdapter`(impl prd/05,text M0)→ normalize。
4. 对外膜:`DisclosurePolicy`/`TerminationPolicy`/`ExternalCommunicationGrant` + representation。
5. LocalDevice + A2A bounded(数据模型 + enforce 点,M0 模拟)。

## 9. 测试

- **S1(总纲 §7.1,入口侧)**:CLI/Gateway 同 `RunRequest`/同 event;入口不拥有 loop。
- **S12–S17(总纲 §7.17–7.22)**:沟通簇全覆盖(标准化入口/外部会话终止/披露阻断/代表身份/设备授权/A2A bounded)。
- **单元**:adapter 不绕 Gateway;外部会话超限终止;披露阻断;代表不伪装;设备授权/撤销;A2A 越界拒。

## 10. 验收

- 双入口同协议;adapter 经 Gateway;外部会话有界;披露阻断;代表身份;设备授权;A2A bounded;S1 + S12–S17 通过。

## 11. 原创性与合规

- gateway/channel adapter/communication session 是通用模式;MCP/A2A 若用协议兼容则记 source/license/copy_risk/implementation_rule/notice。
- **不复制**第三方连接器/gateway 源码、prompt 或平台适配代码。
