# forme — 编码代理作业规则(AGENTS.md)

本仓库是 forme:开源 Rust 主动式个人 Agent 内核。**文档是权威,代码照文档实现。** 任何编码代理动手前必须遵守本文。

## 读取顺序(每个任务开始时)

1. `docs/prd/01-foundation-implementation-prd.md` — 总纲:§9 集成契约(防各自为战)、§10 模板与跨切规范。
2. 当前任务若属于 V1 核心大脑闭合，先读 `docs/prd/23-v1-core-brain-closure-program-prd.md` 对应 A/B/C/D 波；否则读负责模块的 `docs/prd/NN-*.md`。
3. `docs/architecture/canonical-contract.md` §1–29 — 唯一口径;同名对象/判断以它为准，V1 闭合重点为 §28，安装/常驻服务/Web 工作台重点为 §29。
4. `docs/architecture/09-v1-core-brain-runtime-closure-architecture.md` — 当前 composition、持续认知、编排、多模型与数据闭合架构。
5. `docs/architecture/03-foundation-architecture.md` — 冻结 crate 图、事件分类(§2.1.1)、trait 签名(§4.7/§5.6)、治理 enforce 点(§6)。
6. 安装/系统服务/Web 任务另读 `docs/requirements/17-18`、`docs/architecture/10-installed-product-and-web-workbench-architecture.md` 及对应 `docs/prd/24-26`；产品化正在分阶段实施，当前分支/组件/IP 状态见 `docs/acceptance/installed-product-implementation-progress.md`，不是 M6。

## 硬规则(违反 = 返工)

1. **依赖方向即架构**:crate 依赖边只能是 `architecture/03 §1` 表中列出的;禁止新增边、禁止反向依赖。
2. **事件是大脑模块间唯一事实交换**:`EventKind` 只活在 `forme-protocol`;所有稳定脑状态变更必须落事件(append 只经 `forme-store` 单写者);新增事件类型必须先改 architecture §2.1.1 再改代码。安装/认证/服务管理事实按 canonical §29 独立持久化，不属于脑事件，也不得写脑表或推进 run。
3. **PRD 里的 trait 签名与协议对象是冻结契约**:实现它,不重设计;要改先提文档变更,经确认后代码跟进。
4. **原创性(canonical §12)**:严禁复制第三方项目的源码、prompt、错误文案、命名体系、目录结构、测试 fixture；唯一限定例外为 owner 已授权的 DeepSeek 前端 UI，严格按 `docs/compliance/frontend-reuse-policy.md` 逐文件审查/白名单/NOTICE，不含其内核、插件框架、服务端、prompt 或 fixture。`research/upstreams/` 永不引用;每次提交前 `bash tools/compliance-doctor.sh` 必须 PASS。
5. **默认禁止 unsafe**:每 crate 使用 `#![forbid(unsafe_code)]`；唯一获准例外是 canonical §29.9 明列的三个 Windows 原生边界文件：两个 AppContainer adapter 与一个 AgentExecution peer transport adapter。`execution` crate 根与 `installed` 模块仅因此使用 `#![deny(unsafe_code)]`，三个 adapter 文件必须各自显式 `#![allow(unsafe_code)]`；质量门拒绝未审计的其他例外。每个协议对象带 `SchemaVersion`(canonical §9)。
6. **治理不变量不得绕过**(architecture §6):harness-first、执行前重查、能力门在出口、候选先于稳定、trust 不覆盖 permission、审批绑定 plan digest(防 TOCTOU)、`stream_seq` 为排序权威、副作用不明绝不盲重试。
7. **V1 闭合不是 M6**:不新增内核/crate/依赖边；前 97 EventKinds strict prefix，只允许 `WorkspaceCharterChanged`、`DataLifecycleApplied` 末尾 additive；C1–C24 不是 S100+。
8. **历史 M0 边界**:M0 当时的薄化指引（激活扩散/签名学习/自动提升留在后续）只描述历史基线；V1 闭合按 `prd/23` 明确激活的能力实施，不得用历史 M0 口径阻断 B/C/D，也不得借闭合之名回退 M0 治理。
9. **测试**:按模块 PRD §9 + prd/01 §7 的场景规格——事件序列断言(非最终回答)、正反场景、三类正确性;fixture 自建,不抄第三方。
10. **模型事实不得造假**:每次 call 前绑定具体 provider，事件记录实际 profile；禁止内部 fallback 仍自报 primary。
11. **默认大脑不得退化**:正式 CLI/Gateway/scheduler 必须使用同一 ProductionRuntime；不得用空 context、入口私有 memory、NoopProactivity 或测试 builder 冒充完成。
12. **产品壳不能旁路大脑**:Web 不直调模型/工具/脑数据库；系统安装提权不等于 Agent 无限 root/SYSTEM；维护命令与 AgentExecution 分域。关闭网页不停止服务，core 离线仍可受认证维护；产品验收用 IP01–IP24，不借历史 S/C 或跨编译宣称原生平台已通过。

## 当前构建顺序

M0-M5 及 `prd/23` 的 **A 默认装配(C1-C6)** → **B 持续认知(C7-C12)** → **C 项目与编排(C13-C18)** → **D 产品闭合(C19-C24)** 已实现并合入本地 main。当前技术验收、许可证与发布回执状态以 `docs/acceptance/core-brain-closure-acceptance-report.md` 为准；owner 日常使用验收与远端发布分别记录。后续修改继续复跑 S1-S99 与相应 closure 门；不得以 placeholder、受控测试或旧源码回执替代当前完整验收。

## 提交约定

代码维护遵守 `docs/engineering/code-quality.md`；每次实现提交增加 `py -3 tools/code-quality.py`。新增生产文件原则上 ≤600 行，测试 ≤1200 行；现有超限文件按 `tools/quality-baseline.json` 不增长棘轮，不得自动刷新基线或用 include 拼接规避。拆分先保持 public API/事件/事务/恢复行为，产品化不得继续堆进巨型 lib.rs。

构建资源遵守 `docs/engineering/build-resources.md`：沿用根 dev/test 节省空间配置，每个工作树复用固定 target，不反复新建时间戳 target。构建前运行 `py -3 tools/build_resources.py check`；整轮任务结束后保存空间报告。临时/已结束任务的缓存只在无占用且证据已保存时，通过该工具显式预览/应用清理；不得直接整 target 删除或在每个测试后清空。

conventional commits、按 crate scoped；文档与代码分开提交。每个实现提交前：`cargo fmt --all -- --check && cargo check --workspace --all-targets && cargo test --workspace --all-targets && bash tools/compliance-doctor.sh`；波次门另跑 strict clippy、`tools/verify-m5.ps1` 和对应 closure gate。
