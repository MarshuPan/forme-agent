# forme 架构文档目录

本目录存放 forme 的架构文档。它的最终职责是回答“需求如何实现”，包括 Rust 模块、协议、状态机、数据流、存储、安全、权限、模型、工具、扩展和可观测性。

全局文档路线以 `../README.md` 和 `../documentation-map.md` 为准。

## 当前定位

早期架构基线 `00-foundation-direction.md` 与 `01-agent-harness-and-loop.md` 已移入 `../archive/architecture/`，其结论由 `canonical-contract.md` 与 `03-foundation-architecture.md` 承接。本目录现有文档中：

- `canonical-contract.md` 固定正式架构前必须收敛的跨文档契约，包括三内核术语、主动等级、自治边界、稳定性枚举、对象 owner、CompetenceGate、并发一致性、撤销派生失效、schema 版本、身份模型、失败分类、开源原创 CI 门、M2 对外真实动作治理、M3 受控策略进化治理、M4 单 Owner 联邦运行时治理、M5 受治理能力生态和 V1 核心大脑运行闭合。
- `02-proactive-cognitive-kernel.md` 把主动认知内核怎么转、各对象怎么咬合的设计叙事固化为基准（记忆图、认知地图、进化引擎、协调、主动机制、能力门、性格、对外膜、runtime 接缝）；同名对象口径以 `canonical-contract.md` 为准。

`02-proactive-cognitive-kernel.md` 已把主动认知内核的设计结论固化为基准。正式 Rust 实现架构 `03-foundation-architecture.md` 已在 `requirements/01-04` 与该基准之后编写完成（骨架 + 详篇：完整事件分类、承重墙/认知层 trait 签名），并由 `prd/01` 承接为 S1–S22 可执行验收。`requirements/03` 已固定 M0 是完整核心体系的第一条可运行闭环，`requirements/04` 已固定 M0 的验证与验收策略。

M0 完成后，`requirements/05-06` 将 M1 固定为“增强已闭合内核”。`04-m1-experience-architecture.md` 是 M1 的增量技术方案：控制面、trace/manual eval、intention-backed scheduler、notification、context 与 capability 效率；它不替代或放松 `03` 的 M0 承重墙。M1-A 控制/评价面已激活并通过 S23-S28；M1-B 后台/主动工作流已激活并通过 S29-S33；M1-C context/capability efficiency 已激活并通过 S34-S37 与全回归。2026-07-15，真实配置模型 golden trace/eval 与最终验收报告补齐，M1 final PASS。

M2 由 `requirements/07-08` 冻结为“跨系统主动协作”。`05-m2-cross-system-collaboration-architecture.md` 激活 canonical §24，把 Browser/Computer/PTY、AppApi/真实沟通、ResourceGraph/能力成长/managed policy/hot-cold sync 分为 A/B/C 三波。三波均已在 owner review 后实现并通过 S1-S52、真实 Chrome/API、typed artifacts、strict clippy 与合规门；18-crate 图和 86 个 EventKind 保持冻结。M2 final PASS。

M3 由 `requirements/09-10` 定义为“受控自进化”。`06-m3-controlled-evolution-architecture.md` 激活 canonical §25，先建 replay/eval/promotion/activation/rollback 控制面，再分波接入 Loop/Coordination/Selection 与 Self/Partnership/Trust/Proactivity。A/B/C 三波均已通过 owner review 与分波门；M3 final 通过 S1-S69、真实 browser evolution golden、typed artifacts 与 clean-tree release audit。

M4 由 `requirements/11-12` 定义为“单 Owner 联邦运行时”。`07-m4-federated-runtime-architecture.md` 激活 canonical §26，把 peer trust plane/remote execution、multi-peer read-only replication/cross-device control、verified checkpoint continuity 分为 A/B/C 三波。三波已通过 owner 验收并封板；93-kind taxonomy、single authority 与 18-crate 图保持冻结。

M5 由 `requirements/13-14` 定义为“受治理能力生态”。`08-m5-governed-capability-ecosystem-architecture.md` 激活 canonical §27，把 publisher/admission、plan-bound lifecycle/registry、M4-bound distribution/release 分为 A/B/C 三波；三波已通过 S1-S99 工程验收并合入 main，18-crate 图与 single authority 保持冻结。

V1 核心大脑闭合由 `requirements/15-16` 定义，明确不是 M6。`09-v1-core-brain-runtime-closure-architecture.md` 激活 canonical §28，把默认 composition、持续认知、项目/编排闭环和数据/价值闭合分为 A/B/C/D 四波；只允许在 M5 97-kind 后追加两个 R 组事件，不新增 crate、依赖边或内核。

## 当前文档

| 文档 | 作用 | 状态 |
|---|---|---|
| `canonical-contract.md` | 进入正式架构前的唯一口径契约，收敛 L0-L5、AutonomyEnvelope、稳定性、失败分类、对象 owner 和缺失机制。 | 当前有效，后续 `02`/`03` 必须遵守。 |
| `02-proactive-cognitive-kernel.md` | 主动认知内核设计基准（设计叙事），固化记忆图、认知地图、进化引擎、协调、主动机制、能力门、性格、对外膜和 runtime 接缝。 | 当前有效，是后续技术方案的硬约束；口径以 `canonical-contract` 为准。 |
| `03-foundation-architecture.md` | 正式技术方案，定义 Rust 模块、数据流、接口和实现边界，并承接 `requirements/03-04` 的 M0 范围与验收要求。 | 完成：骨架（§0–9）+ 详篇（§2.1.1 完整事件分类、§4.7 承重墙 + §5.6 认知层核心 trait 签名）。协议 struct 的实现级细节留 `prd/01` 或实现期。 |
| `04-m1-experience-architecture.md` | M1 增量技术方案，定义 Local Gateway 控制面、trace/review/manual eval、background scheduler、notification 和按需 context/capability 装载。 | M1-A/B/C 已实现并验收；继续复用 M0 86 个 EventKind 与内部 crate 图。 |
| `05-m2-cross-system-collaboration-architecture.md` | M2 增量架构，定义受治理 Browser/Computer/PTY、connector/真实沟通、ResourceGraph/能力成长/managed policy/hot-cold sync。 | M2-A/B/C 已实现并验收；M2 final PASS。 |
| `m2-a-protocol-compatibility.md` | M2-A backend/action/output-trust/config enum 增量兼容边界。 | 已随 M2-A protocol 激活；保持 86 个 EventKind。 |
| `m2-b-protocol-compatibility.md` | M2-B AppApi/DisclosureBinding/connector config 增量兼容边界。 | 已随 M2-B 激活；legacy 缺 binding 不授权新发送。 |
| `m2-c-protocol-compatibility.md` | M2-C ResourceGraph/goal/growth/managed plugin/hot-cold/CAS sync 增量兼容边界。 | 已随 M2-C 激活；冻结 EventStore 不变，EventKind 保持 86。 |
| `06-m3-controlled-evolution-architecture.md` | M3 增量架构，定义 replay/simulation/eval、四态分离、active CAS/run pinning、domain strategy 与 rollback。 | M3-A/B/C 已实现并验收；M3 final PASS。 |
| `m3-a-protocol-compatibility.md` | M3-A common evolution DTO、三个 EventKind、legacy/default/replay 兼容边界。 | 已随 M3-A 激活；既有 86 严格前缀，当前 89。 |
| `m3-b-protocol-compatibility.md` | M3-B Loop/Coordination/Selection/ModelAdaptation spec 与 forbidden-field 边界。 | 已激活并通过 S58-S62；不新增 EventKind。 |
| `m3-c-protocol-compatibility.md` | M3-C StrategyMemory/Self/Partnership/Trust/Proactivity/Communication spec 边界。 | 已激活并通过 S63-S69；不新增 EventKind。 |
| `07-m4-federated-runtime-architecture.md` | M4 增量架构，定义 single authority、peer grants、TLS RemoteExecutor、lease/fencing、read-only replication、OwnerClient control 和 checkpoint handoff。 | 已实现并通过 S70-S84 自验收；18-crate 图不变，EventKind 从 89 末尾 additive 到 93。 |
| `08-m5-governed-capability-ecosystem-architecture.md` | M5 增量架构，定义 publisher grant、signed package admission、plan-bound lifecycle、registry 接入和 federated distribution。 | M5-A/B/C 已实现并验收；18-crate 图不变，EventKind 从 93 末尾 additive 到 97。 |
| `09-v1-core-brain-runtime-closure-architecture.md` | V1 核心大脑运行闭合架构，定义唯一 production composition、持久 owner 大脑、默认 context、三相 tick、八套编排、多模型真实性、dashboard、备份/加密/删除。 | 文档已冻结，等待按 A→D 实施；不是 M6，目标为 99 EventKinds 与 18-crate 图不变。 |
| `m5-a-protocol-compatibility.md` | M5-A publisher/package/admission 与四个 Q 组事件的兼容边界。 | 已随 M5-A 激活；93-kind exact prefix、legacy/fail-closed contracts PASS。 |
| `m5-b-protocol-compatibility.md` | M5-B install plan/lifecycle/approval 与 registry projection 的兼容边界。 | 已随 M5-B 激活；legacy admission 不产生 Installed/Enabled。 |
| `m5-c-protocol-compatibility.md` | M5-C distribution receipt、M4 binding 与 portable evidence 的兼容边界。 | 已随 M5-C 激活；self-report/legacy receipt 不产生 verified distribution。 |
| `m1-a-protocol-compatibility.md` | M1-A protocol DTO 与 Notification enum 增量的兼容、拒绝和迁移边界。 | 已随 M1-A 落地；是 M1-A protocol migration note。 |
| `m1-b-protocol-compatibility.md` | M1-B intention/claim/job 与 proactive payload 增量的兼容、拒绝和 replay 边界。 | 已随 M1-B 落地；是 M1-B protocol migration note。 |
| `m1-c-protocol-compatibility.md` | M1-C SchemaDigest、optional MCP action field 与 Context doctor row 的兼容和执行拒绝边界。 | 已随 M1-C 落地；保持 86-event taxonomy 与内部 crate 图。 |

## 写作边界

架构文档应该做：

- 把 requirements 转成可实现的模块和协议。
- 定义核心对象、状态机、数据流、存储和错误边界。
- 明确 policy、approval、delegation、coordination、cognition、temporal memory、UserModel、ImportedHistoricalEvidence、failure evidence、tool runtime、context、memory、skills、MCP、plugins、hooks、gateway、communication fabric 和 model routing 的接口关系。
- 明确 M0 如何形成完整核心闭环，哪些能力进入 M0 第一版运行链路，哪些只属于后续产品广度、生态规模或高阶自治。

架构文档不应该做：

- 重新定义产品愿景。
- 绕过 requirements 新增需求。
- 直接复制第三方的代码、结构、prompt、文档或测试。
- 写成实施任务清单；任务拆解应进入 `docs/prd/`。
