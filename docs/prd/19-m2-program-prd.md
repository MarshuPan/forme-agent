# M2 跨系统协作 Program PRD

M2 实施总纲。本文把 `requirements/07-08` 与 `architecture/05` 收敛成三个有序交付波次，固定跨 crate 契约、协议增量、实现步骤、测试和停顿边界。M2 不建立第二条运行链，也不把真实触点当作削弱治理的理由。

## 0. 定位与承上

- **需求**：`requirements/07-m2-scope-decisions.md` D36-D55。
- **验收**：`requirements/08-m2-verification-strategy.md` S38-S52。
- **架构**：`architecture/05-m2-cross-system-collaboration-architecture.md`。
- **唯一口径**：`canonical-contract.md` §1-24，尤其 §6、§7、§15、§23、§24。
- **冻结基线**：M0/M1 18-crate 图、86 EventKinds、S1-S37、M1 final typed artifacts 和 compliance doctor。
- **交付纪律**：A/B/C 每波实现、报告、停下 owner review；本轮不得提前激活下一波。
- **实施状态（2026-07-17）**：M2-A/B/C、S1-S52、真实 Chrome/API golden、typed artifacts、strict clippy 与合规门均 PASS；M2 final 完成。

## 1. 职责与边界

### 1.1 目标

1. 真实 Browser/Computer/PTY 通过既有 `ActionBackend` 和 Harness 咽喉运行。
2. 真实 connector/communication 通过 provider/adapter lifecycle 和对外膜运行。
3. ResourceGraph、长期目标、能力成长、managed policy、hot/cold/sync 保持事件溯源和候选治理。
4. 真实世界 golden 可复核且不含 credential。

### 1.2 不做

- 不新增内部 crate 或依赖边。
- 不默认 L4/L5，不允许 L5 standing grant。
- 不自动 promotion LoopSpec、CoordinationPolicy、TrustProfile、capability/tool policy。
- 不自研浏览器/视觉/桌面规划智能，不做任意 JavaScript 或无限 computer loop。
- 不做 remote executor、marketplace、多租户公网 Gateway、多主同步。

## 2. 全局位置与集成契约

```text
Model / Owner / Adapter proposal
          |
       Harness
          |
 capability/schema -> policy -> external L3/L5 -> approval
          -> CompetenceGate/evidence -> envelope/digest recheck
          |
   ExecutionBackendRegistry
     /       |       \
 Browser  Computer   PTY       (A)
 AppApi   Communication         (B)
          |
 EventStore -> Verification -> CapabilityEvidence -> candidates (C)
```

### 2.1 Crate ownership

| 能力 | owner | Harness 接缝 | 禁止 |
|---|---|---|---|
| additive DTO/Event payload | protocol | 全模块共享 | module-private wire types |
| permission/parameter/managed policy | policy | execution-time evaluate | driver 自行放行 |
| driver/backend/artifact receipt | execution | immutable plan execute | gateway/model 直调 driver |
| approval/competence/provenance/recovery | harness | 唯一行动咽喉 | 第二 loop/runner |
| provider/tool lifecycle | capabilities | resolve/recheck | install=trusted |
| connector/real session/disclosure | communication/gateway | façade -> harness | adapter 直写 memory/store |
| graph/growth semantics | coordination/cognition | read snapshot / emit candidate | score 自动授权 |
| goal/hot-cold/sync facts | memory/store | event + projection | peer 直写 stable projection |

### 2.2 不变量

- 外部 action 默认至少 L3；L5 每次 one-shot owner approval。
- 每个副作用原子步骤有独立 plan digest；批后任何字段变化作废。
- capability evidence 只支持显式 narrow grant，不自动改 permission/trust/spec。
- external output 永远 Untrusted，provenance 由 Harness/Gateway 赋值。
- external input/output 的 source、trust 与 data/instruction treatment 必须贯通到 `ModelRequest`；模型不能自报或提升 `ActionIntent.source`。
- SecretRef-only；ResolvedSecret 不序列化、不 Debug、不进 artifact。
- unknown outcome 不重试；rollback boundary 诚实。
- EventKind additive-only；M2-A 保持 86。

## 3. 协议对象与接口

实现 `architecture/05 §3-5` 的冻结对象。M2-A 必须先落 protocol contract tests，再改 owner crate。

### 3.1 Protocol delta

- M2-A：`BackendKind::{Browser,Computer,Pty}` additive；`AppApi` 到 M2-B 才 additive。
- `SecretRef` 是既有 `CredentialRef` 的 wire-compatible canonical 名；`ExternalInput`、`BrowserActionSpec/Operation`、`ComputerActionSpec/Operation`、`PtyActionSpec`、`SecretBinding`、`ExternalActionReceipt` 均 versioned。
- M2-A：`ActionParameters::{Browser,Computer,Pty}` additive；`AppApi` 到 M2-B 才 additive。
- `ActionOutputDeltaPayload.{trust,content_ref}` 与 `ActionCompletedPayload.receipt` additive，legacy safe default = `Untrusted/None/None`；外部 completion receipt 必须随权威事件持久化。
- M2-A：`ConfigCheck::{Browser,Computer,Pty}` additive；Connector/Sync 分别到 B/C 激活。
- `EventKind::ALL.len()` 在 A 波仍为 86。

### 3.2 Execution trait

```rust
pub trait SecretResolver: Send + Sync {
    fn resolve(&self, reference: &SecretRef) -> Result<ResolvedSecret>;
}

pub trait BrowserDriver: Send + Sync {
    fn perform(&self, action: &BrowserActionSpec, secrets: &dyn SecretResolver)
        -> Result<DriverReceipt>;
}

pub trait ComputerDriver: Send + Sync {
    fn perform(&self, action: &ComputerActionSpec, secrets: &dyn SecretResolver)
        -> Result<DriverReceipt>;
}
```

`BrowserBackend/ComputerBackend/PtyBackend` 实现既有 `ActionBackend`，不修改 trait 形状。Driver receipt 进入 `ResultNormalizer`，由 execution 发 action/capability payload，由 Harness append。

### 3.3 Policy/Harness 约束

- `PolicyContext` additive 增加 browser origins、computer surfaces/bounds、PTY programs/roots 和 external-action posture。
- `execution_recheck` 对 backend/parameters、origin scheme、surface bounds、program/root、secret binding 做 total match；未知组合 deny。
- Harness `required_intervention_level` 对 external backend 给 L3/L5；显式 L4 例外必须同时满足 architecture/05 §5 的全部条件。
- approval request `affected_resources` 覆盖 origin/surface/program/cwd；plan digest覆盖 driver/action/input/SecretRef/scope/timeout/rollback。
- execution sink 的 output event 由 Harness 盖 Untrusted provenance。

## 4. M2-A：对外执行 Backend

### A0 Canonical/Protocol

1. 增补 canonical §24，冻结 D36-D55/S38-S52/architecture/05/本 PRD。
2. 增加 protocol DTO/enums、serde round-trip、invalid-boundary、legacy payload、86-kind snapshot tests。
3. 新建 `architecture/m2-a-protocol-compatibility.md`。

### A1 External Action Guard

1. Policy 参数重查覆盖 Browser/Computer/PTY。
2. Harness external guard：L3 default、L5 one-shot、narrow L4 的 explicit envelope+evidence+competence 条件。
3. approval affected resources/digest/recheck；stale plan deny。
4. output provenance override；unknown outcome recovery 不变。

### A2 Common Driver/Artifact Boundary

1. `SecretResolver/ResolvedSecret`，ResolvedSecret 不可 Debug/Serialize/Clone。
2. `DriverReceipt/EffectStatus` 和 bounded safe summary。
3. scoped artifact root：canonical path、no escape、atomic create、content digest；event 只存 ref。
4. recording driver 只用于 unit/negative tests，不能冒充 real golden。

### A3 BrowserBackend

1. `BrowserBackend` 负责 plan/budget/timeout/cancel/events/evidence。
2. `HeadlessChromeDriver` 使用 pinned `headless_chrome`，只做 Navigate/ReadText/Click/Type/Screenshot。
3. origin allowlist、scheme、selector/input limit；禁 JS/file/data/download execute。
4. real golden 启动 repository-owned loopback HTTP fixture，真实 Chrome/Edge click 产生服务端 mutation receipt。

### A4 ComputerBackend

1. `ComputerBackend` + `ComputerDriver`，动作严格绑定 surface/bounds。
2. `NativeComputerDriver` 使用 pinned `enigo`；Screenshot 使用 pinned `xcap` 写 artifact ref。
3. production driver 默认不注册；必须显式 config/allowed surface。
4. deterministic tests 用 isolated/recording driver 验证参数和旁路不存在；不在测试中控制 owner 当前桌面。

### A5 PtyBackend

1. pinned `portable-pty` 提供真实 PTY。
2. program/root/size/input/env/timeout/output/cancel 限制；不继承环境、不 fallback shell。
3. repository-owned fixture 验证终端输入输出、timeout/cancel 和失败终态。

### A6 Config/Compliance/Golden

1. ConfigDoctor 增加 Browser/Computer/Pty finding。
2. 精确 pin 四个 driver 依赖和 `url` 解析依赖，并更新 third-party table/borrowing record；检查 license/MSRV/Windows GNU。
3. 新建 M2-A golden case、typed report、portable trace manifest 和 artifact validator。
4. `tools/verify-m2.ps1` 先跑 verify-m1，再跑 S38-S42 和 artifact gate。

### M2-A gate

S38-S42 + S1-S37 + 86 EventKinds + crate graph + fmt/check/strict clippy/all tests/compliance + real browser golden。完成后生成 `docs/acceptance/m2-a-acceptance-report.md` 并停下 owner review。

## 5. M2-B：Connector 与真实沟通（已实现并验收）

### B0 AppApi Framework

- `AppApiConnector: ExternalProvider`，lifecycle/identity/SecretRef/schema/rate/timeout/revocation。
- read/mutation 都转 `ActionIntent{backend=AppApi}`；mutation 默认至少 L3。
- 激活一个 project-owned limited connector，未知 outcome 不重试。

### B1 Real Communication Adapter

- 至少一个真实 adapter 只 normalize/transport，所有 send 经 Harness action。
- ExternalCommunicationGrant、TTL/budget/termination/transcript policy 真实 enforce。
- provenance 由 Gateway 盖章。
- ExternalCommunicationGrant 创建必须带 owner `AuthContext`，grant event 为 owner provenance，session lifecycle event 为 verified process provenance。

### B2 Disclosure/Representation/Device/A2A

- DisclosurePolicyApplied 必须先于 delivery，并以 additive `DisclosureBinding` 精确绑定 session/participant/purpose/content/policy inputs；legacy `None` 只可 replay、不可授权新发送。
- representing owner 不伪装 owner；高影响逐次审批。
- DeviceGrant/revocation/retention 与 bounded A2A 强终止进入真实 adapter tests；`retain_raw=false` 必须以 store/FTS 无原文和 TTL 后拒绝证明，不能只检查内存标志。

### M2-B gate

S43-S47 + M2-A/M1/M0 全回归，生成 `m2-b-acceptance-report.md` 并停下 owner review。

## 6. M2-C：认知/协调/持久化加深（已实现并验收）

### C0 ResourceGraph

- 从 events/CapabilityEvidence/Verification/Failure 派生可重建 graph。
- score 带 scope/freshness/evidence；DecisionTrace 引用 snapshot；score 不授权。
- Protocol 机械契约先冻结在 `architecture/m2-c-protocol-compatibility.md`；EventKind 保持 86。

### C1 Long-term Goal

- GoalFrame/ProspectiveIntention/ExecutionRoute/checkpoint artifacts 形成长期 lineage。
- 前台优先、budget/cancel/revoke、情境变化重议。
- 每次 continuation 仍是普通 ScheduleCommand/Harness run，不建立 private scheduler truth。

### C2 Capability Growth

- CapabilityGap/CapabilityUpdateProposal candidate。
- owner review 后只产生 narrow explicit grant；不改默认 loop/coord/trust。
- 结果 pass/fail/unverifiable/owner feedback 主导，自评只能压低 proposal ceiling。

### C3 Managed Plugin Policy

- managed layer source/signature/allowlist/deny/revoke。
- contribution snapshot 验证后原子切换，managed deny 不可放宽。
- reload 校验失败保留旧 generation；revoke 后 tool/skill/MCP/hook/config 全部不可见。

### C4 Hot/Cold + One-peer Sync

- 同一 event source 的 hot/cold projection、selective recall、retention/redaction。
- 一个 peer 在 store open 时由 `StoreOptions` 显式配置，默认未配置即禁用 sync；首次请求不能 TOFU 绑定，已有数据库与新配置语义不一致时 open fail closed。
- expected-version/CAS、idempotent batch、conflict reporting；SecretRef 不出同步 artifact。
- 冻结 EventStore trait 不改；新增 VersionedEventStore additive trait，batch compare/write/cursor 同一事务。

### M2-C gate

S48-S52 + M2-A/B/M1/M0 全回归，生成 `m2-c-acceptance-report.md` 并停下 owner review。

## 7. 错误、安全、权限与审批

| 失败 | 分类/结果 |
|---|---|
| backend/parameter mismatch | safety policy failure；deny before ActionStarted。 |
| origin/surface/program/root 越界 | policy deny；不调用 driver。 |
| external output injection/伪 owner | Untrusted quarantine；必要时 safety policy evidence。 |
| credential missing/leak attempt | config/safety failure；fail closed；artifact gate fail。 |
| stale digest/schema/scope/SecretRef | approval invalid；重新规划/审批。 |
| driver timeout/cancel | ActionFailed/Cancelled + CapabilityEvidence。 |
| side effect outcome unknown | ActionOutcomeUnknown + waiting/manual/probe；no retry。 |
| rollback 不成立 | non-retractable + L5；补救不标 rollback。 |
| capability evidence 不足 | competence 降级到 L3/L2/L1/L0，不抬权限。 |

## 8. 配置与可观测

实现 architecture/05 §10 的 browser/computer/PTY/external action keys。所有 backend 默认 disabled；ConfigDoctor 必须报告 driver availability、allowlist/root/surface、artifact permissions、secret resolver、approval posture 和阻断性。

M2-A trace 必须能解释：为什么选择 backend、为什么至少 L3/L5、用了哪些结果证据、哪个 plan 被批准、外部内容为什么是 Untrusted、真实地面结果是什么。不得展示模型隐推理或 ResolvedSecret。

## 9. 测试策略

### 9.1 单元

- protocol round-trip/invalid/legacy/default trust/86 snapshot。
- origin/surface/bounds/program/root/secret binding validation。
- plan digest 覆盖全部外部动作字段。
- L3/L5/narrow-L4 判定与 result-evidence priority。
- browser/computer recording driver；PTY real fixture；artifact no-escape/digest。

### 9.2 集成

- Scripted model -> Harness -> approval -> execution -> verification -> capability evidence。
- deny/expired/stale/cancel/timeout/duplicate/unknown outcome 反向路径。
- external output envelope provenance = Untrusted；injection 不进 stable/graph。
- real headless browser loopback mutation golden；artifact secret scan。

### 9.3 场景门

严格实现 `requirements/08` S38-S52。A 波只激活 S38-S42；verify-m2 组合已激活 M2 场景、完整 M1 gate、protocol/crate graph check、real-world artifact gate 和 compliance。

## 10. 验收

M2 完成条件沿用 `requirements/07 §7`。每波完成必须：

1. 当前波 S# + 所有前序 S# 全绿，无 ignored/placeholder 集成场景。
2. 事件序列、store/projection、反向断言和真实地面结果可复核。
3. fmt/check/strict clippy/all tests/compliance PASS。
4. dependency/borrowing records 完整，SecretRef/artifact scan PASS。
5. 生成对应 `docs/acceptance/m2-*-acceptance-report.md` 并停下 owner review。

M2 final 另需 `m2-acceptance-report.md`，证明真实跨系统行动没有削弱任何 M0/M1 不变量。

## 11. 原创性与合规

- Browser/Computer/PTY 只使用第三方公开 API，并在可替换 driver trait 后自主实现 forme adapter；不复制参考 Agent 项目的 backend、prompt、错误、目录或 fixture。
- `headless_chrome`、`enigo`、`xcap`、`portable-pty` 精确 pin，记录 source/license/copy risk/implementation rule/notice。
- real golden 页面、PTY fixture、connector fixture 和注入文本全部 repository-owned。
- 每次提交前 compliance doctor 必须 PASS；失败即不得进入下一波或生成 PASS 报告。
