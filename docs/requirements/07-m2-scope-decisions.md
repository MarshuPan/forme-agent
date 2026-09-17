# M2 跨系统协作范围决策

本文承接 `02-capability-requirements.md`、`03-foundation-scope-decisions.md`、`05-m1-scope-decisions.md`、M1 最终验收报告和 `architecture/canonical-contract.md` §24，冻结 forme M2 的范围。M2 是**跨系统主动协作**：让已经闭合、可日常使用的内核第一次作用于真实浏览器、桌面、交互式进程、API 和外部参与者，同时保持所有 M0/M1 治理不变量。

M2 不是“工具数量扩张”，也不是进入 M3 自进化。它要证明：**forme 能跨真实系统行动，而且行动主权、凭据、来源信任、可逆性和地面真值仍由 Agent 侧治理。**

## 1. M2 目标

1. Browser、Computer 和 PTY 成为 `execution` crate 中受治理的 `ActionBackend`，只经 Harness 咽喉执行。
2. App API connector 与真实 communication adapter 复用统一 provider lifecycle，并把 M0/M1 的对外膜真正落实到外部参与者。
3. ResourceGraph、长期目标、能力成长、managed plugin policy、热冷记忆与多设备同步在候选、证据和单写者边界内闭合。
4. 至少一个真实 browser 或真实 API golden task 生成可离线复核、无 secret 的 typed eval 与 portable trace manifest。
5. S1-S37、现有 EventKind/内部 crate 图、全量测试和合规门永久不回退。

## 2. 范围判据

- **结构不缺席、广度可收**：每个 M2 域要有真实第一条链，但首波只支持有限 operation、有限 adapter、有限同步拓扑。
- **治理先于触点**：真实副作用必须先有 canonical §24、protocol、plan digest、approval、CompetenceGate、provenance 和 unknown-outcome 边界，再接 driver。
- **模型潮线**：Browser/Computer 的“智能”位于 canonical §23 潮线之下，只薄建 driver trait 和地面真值捕获；资源所有权、跨时间、治理和审计做深。
- **结果证据主导**：能力成长只读 CapabilityEvidence、verification、FailureEvidence 和 owner feedback；自评不能抬高自治上限。
- **M3 边界不越**：M2 可以形成知识、能力、delegation、coordination 的候选和显式 owner grant，但不自动 promotion 默认 LoopSpec、CoordinationPolicy、TrustProfile、managed policy 或 autonomy strategy。

## 3. 范围决策

| 编号 | 主题 | M2 决策 | 不进入 M2 核心的部分 |
|---|---|---|---|
| D36 | 阶段定义 | M2 是跨系统主动协作；M0/M1 的 18-crate 受治理内核和 S1-S37 视为冻结基线。 | 重做内核、为外部触点建立第二条 loop/store/policy。 |
| D37 | 对外动作不变量 | 激活 canonical §24：真实外部效果默认至少 L3；L5 永远逐动作显式审批；每个副作用步骤绑定不可变 plan digest。M1 owner-local notification 继续走已冻结的本地投递 guard，不能被泛化成其他 outward backend 的例外。 | 默认 L4/L5、开放会话后任意动作、trust 覆盖 permission。 |
| D38 | 协议与依赖 | M2 protocol 只 additive；优先复用 86 个 EventKind；不新增内部 crate 或依赖边。新稳定对象带 `SchemaVersion`，legacy payload 有兼容测试和 migration note。 | 模块私有事件、adapter 私有事实库、反向依赖。 |
| D39 | BrowserBackend | 在 `execution` 中增加薄 `BrowserBackend` + `BrowserDriver`；M2-A 只做导航、受限 DOM 读取、selector click/type 和 screenshot/ref，origin allowlist、超时、输出预算和逐步骤 plan-bound。 | 浏览器推理栈、无限会话、任意脚本执行、下载后自动执行。 |
| D40 | ComputerBackend | 在 `execution` 中增加薄 `ComputerBackend` + `ComputerDriver`；M2-A 只做显式 surface 上的 move/click/type/key/scroll，坐标/按键/文本引用全部在 plan 中，默认 ask。 | 常驻看屏、后台键鼠监听、无 surface 绑定的全桌面自治。 |
| D41 | PtyBackend | 在 `execution` 中增加 `PtyBackend`，支持受限程序、cwd、尺寸、输入引用、输出预算、timeout/cancel 和真实 PTY 终态；不 fallback shell。 | 无期限交互 shell、继承全部环境、未授权网络、PTY 代替 sandbox。 |
| D42 | 外部观察与防投毒 | Browser/Computer/PTY/API/外部参与者输出由 Harness 盖 `Untrusted` provenance；内容只作数据，进入 context 前保留 source/trust/quarantine 边界。 | adapter/content 自报 owner/trust、未信任内容直接改稳定认知或图边。 |
| D43 | 凭据 | credential 只以 `SecretRef` 存于 protocol/config/plan；执行边界按需解析、禁止 Debug/事件/日志/trace/eval 回显。 | 明文 key、把 token 放 args/URL/payload、在 artifact 中保留 endpoint+credential。 |
| D44 | 结果、幂等与恢复 | 每次真实动作记录 started/terminal/verification/CapabilityEvidence；副作用不明进入 `ActionOutcomeUnknown`，只做可证明 probe 或人工裁决。 | lease 到期即重试、浏览器/API mutation 盲重放。 |
| D45 | AppApi connector | M2-B 增加 AppApi connector framework，复用 `ExternalProvider` lifecycle、SecretRef、identity/scope、schema、rate/timeout、policy 和 `ActionBackend` 执行。首批只激活有限 project-owned connector。 | 任意公网 API、connector 自带权限、远端 marketplace。 |
| D46 | 真实 communication adapter | M2-B 将 S13-S17 的数据边界接到至少一个真实 adapter：真实 external counterparty、TTL/budget、termination、transcript policy、approval delivery。 | 无限外部会话、默认外发、公开多租户 Gateway。 |
| D47 | Disclosure 与身份 | 每次真实对外表达都先 `DisclosurePolicyApplied`；agent representing owner 不伪装 owner；高影响/未确认内容逐动作审批。 | 低置信候选当事实对外、外部内容冒充 owner 指令。 |
| D48 | ResourceGraph | M2-C 建立 event-derived ResourceGraph，记录 tool/skill/model/memory/source/backend 的可用性、scope、证据和结果评分；Coordination 只读 snapshot/ref。 | ResourceGraph 自己执行、评分自动改 permission/trust。 |
| D49 | 长期目标 | 长期目标落为 versioned goal/intention/artifact lineage，由 scheduler/tick 持续推进，前台优先，情境变化时重议。 | fire-and-forget 长任务、后台覆盖前台、无终止条件的自治项目。 |
| D50 | 能力成长 | `CapabilityGap -> CapabilityUpdateProposal -> owner review -> narrow grant`；CapabilityEvidence/verification/failure 为主证据。 | 自动安装能力、自动改默认 tool selection/loop/coordination/trust。 |
| D51 | Managed plugin policy | M2-C 增加 managed policy 层的签名/来源/allowlist/deny/撤销和 contribution snapshot enforcement；managed deny 不可被下层放宽。 | 远端自动更新、动态 ABI、插件市场。 |
| D52 | 热冷记忆 | M2-C 增加 event-derived hot/cold tier、selective recall、retention/redaction 和可重建索引；冷层不是第二事实源。 | 把冷归档当独立稳定真相、全量个人数据湖。 |
| D53 | 多设备同步 | M2-C 首版只支持单 owner、中心事实源、一个同步 peer 的增量 event transfer；aggregate expected-version/CAS、防冲突和 SecretRef redaction 必须先成立。 | 多主无冲突同步、多租户、远端直接写 stable projection。 |
| D54 | EventKind | M2-A 默认不新增 EventKind，复用 Action/Policy/Approval/Verification/Capability 事件并 additive 扩 payload；B/C 若事实无法表达，先改 architecture taxonomy。 | 仅为页面/driver convenience 新增重复事件。 |
| D55 | 交付波次 | M2-A 对外执行 backend -> M2-B connector/真实沟通 -> M2-C 认知/协调/记忆加深。每波独立报告并停下 owner review。 | 横向同时实现三波、未验收就提前激活下一波。 |

## 4. 三波交付范围

### M2-A：对外执行 backend

- BrowserBackend/BrowserDriver。
- ComputerBackend/ComputerDriver。
- PtyBackend。
- 外部动作默认 L3/L5 floor、plan digest、approval、CompetenceGate、rollback/unknown outcome。
- 外部 observation `Untrusted` provenance 与 SecretRef artifact scan。
- S38-S42 + S1-S37 全回归 + 真实 browser golden。

### M2-B：连接器与真实沟通

- AppApi connector framework + 首个有限 connector。
- 至少一个真实 ChannelAdapter/外部参与者路径。
- S13-S17 的 grant/disclosure/termination/representation/device/A2A 边界在真实 adapter 上 enforce。
- SecretRef、connector identity、scope、rate/timeout、revocation。
- S43-S47 + A/M1/M0 全回归。

### M2-C：认知、协调与持久化加深

- ResourceGraph 与结果证据评分。
- 长期 goal/intention/artifact lineage。
- CapabilityGap/CapabilityUpdateProposal + owner-reviewed narrow grant。
- managed plugin policy。
- 热冷记忆 + 单 peer 多设备同步 + expected-version/CAS。
- S48-S52 + A/B/M1/M0 全回归。

## 5. M2 非目标（留 M3 或更后）

- LoopSpec、CoordinationPolicy、TrustProfile、managed policy、tool selection policy 的自动 replay/eval/promotion/rollback。
- 自动扩大 autonomy、自动批准高影响动作、L5 standing grant。
- browser/computer 的自研推理、视觉 grounding、规划模型或无限 autonomic loop。
- remote executor、公开 connector marketplace、plugin marketplace、多租户公网 Gateway。
- 多主无冲突同步、组织级身份与审计。
- 激活扩散作为 M2 完成条件；它仍须先打赢 canonical §22 的直接模型基线。

## 6. 成功指标

- **治理**：Harness bypass = 0；真实外部动作未经所需 approval 的执行数 = 0；L5 standing authorization = 0。
- **安全**：外部输入 provenance 错标 = 0；secret 出现在 event/log/trace/eval artifact = 0；prompt-injection fixture 改写稳定认知/图边 = 0。
- **可靠性**：plan digest mismatch 执行数 = 0；unknown outcome 盲重试 = 0；相同 intent 重复副作用 = 0。
- **能力**：Browser/Computer/PTY 各有正反事件序列；至少一个真实 browser/API golden 可离线核验。
- **回归**：S1-S37、全部现有测试、fmt、strict clippy 和 compliance 每波全绿。

## 7. M2 完成定义

1. S1-S37 全绿，`08-m2-verification-strategy.md` 的 S38-S52 全绿。
2. A/B/C 每波都有事件序列、store/projection、反向断言和 `docs/acceptance/m2-*-acceptance-report.md`。
3. EventKind 只 additive、内部 crate 图无新增/反向边、所有新协议对象 versioned。
4. 至少一个真实 browser 或 API golden 产生 typed eval、portable trace manifest 和 secret scan PASS。
5. fmt、check、strict clippy、workspace tests、compliance doctor 全部 PASS。
6. 输出 `docs/acceptance/m2-acceptance-report.md`，证明跨真实系统行动没有削弱 M0/M1 不变量。

## 8. 下游约束

- `architecture/05-m2-cross-system-collaboration-architecture.md` 必须逐项实现 D36-D55，不得扩大到 M3。
- `prd/19-m2-program-prd.md` 必须按 A/B/C 顺序映射 S38-S52，并在每波完成后停下 owner review。
- 任何新 EventKind、内部 crate edge、外部动作等级例外或多写者路径，必须先修改 canonical/architecture 并单独评审。
