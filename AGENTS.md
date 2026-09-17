# forme — 编码代理作业规则(AGENTS.md)

本仓库是 forme:开源 Rust 主动式个人 Agent 内核。**文档是权威,代码照文档实现。** 任何编码代理动手前必须遵守本文。

## 读取顺序(每个任务开始时)

1. `docs/prd/01-foundation-implementation-prd.md` — 总纲:§9 集成契约(防各自为战)、§10 模板与跨切规范。
2. 当前任务若属于 V1 核心大脑闭合，先读 `docs/prd/23-v1-core-brain-closure-program-prd.md` 对应 A/B/C/D 波；否则读负责模块的 `docs/prd/NN-*.md`。
3. `docs/architecture/canonical-contract.md` §1–28 — 唯一口径;同名对象/判断以它为准，V1 闭合重点为 §28。
4. `docs/architecture/09-v1-core-brain-runtime-closure-architecture.md` — 当前 composition、持续认知、编排、多模型与数据闭合架构。
5. `docs/architecture/03-foundation-architecture.md` — 冻结 crate 图、事件分类(§2.1.1)、trait 签名(§4.7/§5.6)、治理 enforce 点(§6)。

## 硬规则(违反 = 返工)

1. **依赖方向即架构**:crate 依赖边只能是 `architecture/03 §1` 表中列出的;禁止新增边、禁止反向依赖。
2. **事件是模块间唯一事实交换**:`EventKind` 只活在 `forme-protocol`;所有稳定状态变更必须落事件(append 只经 `forme-store` 单写者);新增事件类型必须先改 architecture §2.1.1 再改代码。
3. **PRD 里的 trait 签名与协议对象是冻结契约**:实现它,不重设计;要改先提文档变更,经确认后代码跟进。
4. **原创性(canonical §12)**:严禁复制任何第三方项目的源码、prompt、错误文案、命名体系、目录结构、测试 fixture;`research/upstreams/` 永不引用;每次提交前 `bash tools/compliance-doctor.sh` 必须 PASS。
5. **每 crate `#![forbid(unsafe_code)]`**;每个协议对象带 `SchemaVersion`(canonical §9)。
6. **治理不变量不得绕过**(architecture §6):harness-first、执行前重查、能力门在出口、候选先于稳定、trust 不覆盖 permission、审批绑定 plan digest(防 TOCTOU)、`stream_seq` 为排序权威、副作用不明绝不盲重试。
7. **V1 闭合不是 M6**:不新增内核/crate/依赖边；前 97 EventKinds strict prefix，只允许 `WorkspaceCharterChanged`、`DataLifecycleApplied` 末尾 additive；C1–C24 不是 S100+。
8. **历史 M0 边界**:M0 当时的薄化指引（激活扩散/签名学习/自动提升留在后续）只描述历史基线；V1 闭合按 `prd/23` 明确激活的能力实施，不得用历史 M0 口径阻断 B/C/D，也不得借闭合之名回退 M0 治理。
9. **测试**:按模块 PRD §9 + prd/01 §7 的场景规格——事件序列断言(非最终回答)、正反场景、三类正确性;fixture 自建,不抄第三方。
10. **模型事实不得造假**:每次 call 前绑定具体 provider，事件记录实际 profile；禁止内部 fallback 仍自报 primary。
11. **默认大脑不得退化**:正式 CLI/Gateway/scheduler 必须使用同一 ProductionRuntime；不得用空 context、入口私有 memory、NoopProactivity 或测试 builder 冒充完成。

## 当前构建顺序

M0-M5 已完成并合入 main。当前按 `prd/23`：**A 默认装配(C1-C6)** → **B 持续认知(C7-C12)** → **C 项目与编排(C13-C18)** → **D 产品闭合(C19-C24)**。每波独立提交/报告并复跑 S1-S99；不得提前用后波 placeholder 宣称前波完成。

## 提交约定

conventional commits、按 crate scoped；文档与代码分开提交。每个实现提交前：`cargo fmt --all -- --check && cargo check --workspace --all-targets && cargo test --workspace --all-targets && bash tools/compliance-doctor.sh`；波次门另跑 strict clippy、`tools/verify-m5.ps1` 和对应 closure gate。
