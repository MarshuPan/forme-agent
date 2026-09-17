# forme PRD 文档目录

本目录用于存放 forme 的实施型 PRD。

这里的 PRD 不是传统产品经理文档，而是“功能描述 + 实施步骤 + 技术约束 + 测试 + 验收标准”的执行文档。它的目标是让后续实现可以直接按文档推进，减少跑偏、漏做和做错。

全局文档路线以 `../README.md` 和 `../documentation-map.md` 为准。

## 准入条件

编写 PRD 前需要先完成：

- `docs/requirements/01-vision-and-scope.md`：固定愿景、目标、用户、范围、非目标。
- `docs/requirements/02-capability-requirements.md`：固定能力域、优先级和验收方向。
- `docs/requirements/03-foundation-scope-decisions.md`：固定 M0 是完整核心体系的第一条可运行闭环。
- `docs/requirements/04-verification-and-acceptance-strategy.md`：固定 M0 的验证矩阵和验收证据要求。
- `docs/architecture/03-foundation-architecture.md`：固定 Rust 模块、协议、数据流和治理边界。

如果这些前置文档没有完成，PRD 只能写目录说明，不能开始拆实施任务。

## PRD 应包含

- 功能目标和用户价值。
- 功能边界：做什么、不做什么。
- 依赖的需求条目和架构条目。
- 关键对象、状态、事件和数据结构。
- 实施步骤和顺序。
- 错误处理、安全、权限和审批要求。
- 测试策略。
- 验收标准。
- 原创性与合规边界检查要求；如果实现引入依赖、代码复用或协议兼容，再记录具体来源。

## PRD 体系(一纲多目)

系统极复杂、功能极多,PRD 拆成 **1 份总纲 + 每个大模块 1 份实施级 PRD**。总纲统筹全局并定义"集成契约(防各自为战)"与模块模板;模块 PRD 写到"照着能一字不落实现"。模块清单与依赖见 `01` 的 §9.4。

| 文档 | 作用 | 状态 |
|---|---|---|
| `01-foundation-implementation-prd.md` | **总纲(Program PRD)**:不降级承诺、构建顺序、全局验收 S1–S22、集成契约、跨切规范、模块索引与模板。 | 完成。 |
| `02`..`17`(模块 PRD) | 每个大模块一份实施级 PRD(协议/harness/policy/接入框架/记忆/能力/执行/context/认知/时间化记忆/协调/主动/入口沟通/失败验证/模型/合规)。 | `02`–`17` **全部完成**(协议+事件存储 / harness+loop / policy+approval+delegation / 接入框架 / 记忆图 / 能力基质 / 执行 backend / context / 认知 / 时间化记忆+UserModel / 协调+编排+subagent / 主动+能力门 / 入口沟通 / 失败验证 / 模型+配置 / 原创合规)。 |
| `18-m1-program-prd.md` | M1 Program PRD：Local Gateway 控制/评价面、后台/主动工作流、context/capability 效率，以及 S23–S37 分波次门。 | 完成：M1-A/B/C、S1-S37、真实模型 golden 与最终验收均 PASS。 |
| `19-m2-program-prd.md` | M2 Program PRD：Browser/Computer/PTY、connector/真实沟通、ResourceGraph/能力成长/managed policy/hot-cold sync，以及 S38–S52 分波次门。 | M2-A/B/C、S1-S52、真实世界 golden 与最终验收均 PASS。 |
| `20-m3-program-prd.md` | M3 Program PRD：replay/eval/promotion/activation/rollback 控制面、Loop/Coordination/Selection 与 Self/Partnership/Trust/Proactivity 策略进化，以及 S53–S69 分波门。 | 完成：M3-A/B/C、S1-S69、真实 evolution golden 与 release audit 均 PASS。 |
| `21-m4-program-prd.md` | M4 Program PRD：peer trust plane/RemoteExecutor、multi-peer read-only replication/OwnerClient control、checkpoint continuity，以及 S70–S84 分波门。 | M4-A/B/C 与 owner 最终验收 PASS，已封板。 |
| `22-m5-program-prd.md` | M5 Program PRD：publisher/admission、plan-bound lifecycle/registry、federated distribution/release，以及 S85–S99 分波门。 | 完成：M5-A/B/C、S1-S99、真实 ecosystem golden、typed artifacts 与 release audit 均 PASS。 |
| `23-v1-core-brain-closure-program-prd.md` | V1 核心大脑闭合 Program PRD：默认 composition、持续认知、长期项目/编排、多模型、dashboard、数据保护与 C1–C24。 | 文档已冻结，A/B/C/D 实施待完成；明确不是 M6。 |

## 实施状态

M0 已按上述 PRD 实现并完成验收。workspace 包含 18 个 crate，164 个 Rust 测试和 8 个合规 fixture；总纲 §7 的 S1–S22 均有可执行事件序列证据，最终 upstream/license/copy 合规检查均为 PASS。验收报告见 `../acceptance/m0-acceptance-report.md`。

`01`–`17` 继续作为 M0 的冻结实施契约和回归依据。M1 不直接改写这些文档来扩大 M0，而是从新的 M1 requirements、verification strategy、architecture 和 Program PRD 建立下一条实施链。

M1-A 已完成，验收报告见 `../acceptance/m1-a-acceptance-report.md`。M1-B 已完成，验收报告见 `../acceptance/m1-b-acceptance-report.md`。M1-C 已完成，验收报告见 `../acceptance/m1-c-acceptance-report.md`。2026-07-15，真实配置模型 golden task 产生完整 trace/eval report，S1-S37 与合规门复跑全绿；最终报告见 `../acceptance/m1-acceptance-report.md`。**M1 整体完成。**

M2 已沿 `requirements/07-08 -> architecture/05 -> prd/19` 文档链完成。A/B/C 三波分别通过 owner review 后实施，2026-07-17 最终 S1-S52、真实 Chrome/API/communication、typed artifacts、strict clippy 与合规门全绿；逐波和最终报告位于 `../acceptance/m2-*.md`。**M2 整体完成。**

M3 已沿 `requirements/09-10 -> canonical §25 -> architecture/06 -> prd/20 -> acceptance` 完整闭合。A/B/C 三波依次通过 owner review 后实现，S1-S69、89 EventKinds、18-crate 图、309+2 Rust tests、真实 browser evolution golden、typed artifacts、strict clippy、原创门与 release audit 全绿；最终报告见 `../acceptance/m3-acceptance-report.md`。**M3 整体完成。**

M4 已沿 `requirements/11-12 -> canonical §26 -> architecture/07 -> prd/21 -> acceptance` 完成 A/B/C 实现。单 owner/single authority 的受认证远端执行、只读复制、跨设备控制、checkpoint 连续性与三进程 golden 均进入同一治理链；S1-S84、93 EventKinds、18-crate 图、350+3 Rust tests、21 个 Python gate、strict clippy、原创门与 clean-tree release audit 全绿。最终报告见 `../acceptance/m4-acceptance-report.md`；已封板。

M5 已沿 `requirements/13-14 -> canonical §27 -> architecture/08 -> prd/22 -> acceptance` 完整闭合。A/B/C 已完成 publisher/admission、plan-bound lifecycle/registry 与 M4-bound distribution/release；S1-S99、97 EventKinds、18 crates、379+4 Rust tests、29 个 Python gates、真实三角色 golden、deterministic typed artifacts、strict Clippy 与 clean-tree release audit 全绿。最终报告见 `../acceptance/m5-acceptance-report.md`，并已合入 main。

V1 核心大脑闭合已完成 `requirements/15-16 -> canonical §28 -> architecture/09 -> prd/23` 文档冻结。下一步按 A 默认装配、B 持续认知、C 项目/编排、D 产品闭合实施；每波必须复跑 S1-S99，最终 C1-C24、99 EventKinds、18 crates、真实 golden 与 release gate 全绿。

## 写作边界

PRD 不能新增未进入 requirements 的能力，也不能绕过 architecture 自行决定实现方式。所有实现必须原创，不能复制第三方源码、文档表达、prompt、命令体系或测试 fixture。M0 的任务拆解必须覆盖 `requirements/03` 的完整核心闭环和 `requirements/04` 的验收矩阵，不能把核心子系统降级成“后续再补”。
