# V1 核心大脑闭合 B 波（持续认知）分支验收报告

- 日期：2026-10-17。
- 分支：`shu/v1-core-brain-a`；基线 `main@94fe6f1`，本波未合入 `main`。
- 权威规格：`requirements/16` C7–C12、canonical §28、`architecture/09` §5、`prd/23` §4 B0–B6。
- 结果：**B0–B6 分支实现及 C7–C12 专项自检 PASS；整波交付同时以 §4 的干净树 M5 回归与其后的独立 release receipt 为依据。待 owner 验收。** C/D 与 V1 Final 不在本报告的通过范围。

## 1. 模块与提交

| 模块 | 实现提交 | 已接入的行为 |
|---|---|---|
| B0 稳定认知 | `e6b754f`、`1e97fdd`、`b19e067`、`44e974f` | 晋升记录携带对象、scope、证据日期与最近复现时间；在线和 replay 共用 fold，历史无记录载荷继续可读 |
| B1 三相 tick | `c6066d0` | 持久 claim、固定范围/快照/判断时间、Observe → Reflect → Maintain、前台让路、故障接管与重试上限 |
| B2 AskToLearn | `9064e1b` | 根据目标声明的判断依赖发现缺口；owner 回答、拒绝、延后均持久；回答只生成 User/CognitiveMap candidate |
| B3 AgentWorkspace | `84a7645` | owner 全流投影、六类注意力项、scope 先过滤、价值×紧迫排序、硬容量、完整 cursor/digest、正式 context 与 Gateway 接入 |
| B4 相关性路径 | `b536bff` | 五类激活形态、有界扩散、事件化图维护、可替换直接模型路径、实际 provider/usage 记录及离线对照验证 |
| B5 认知维护 | `2ca0653` | 证据过龄、未复现、可信冲突、传递派生重评、低价值分支衰减/修剪；旧快照不能覆盖新知识 |
| B6 整波门禁 | `bf1b800` | 48 个按名场景、Gateway 进程重启证据、CognitiveMap 回答/正式 context、维护健康前缀漏检修复、仓库工件校验 |

设计增量先于实现落在 `ae89d9e`、`d7ce28d`、`74c047c`；B1 模块报告 `bdb39fe` 保留为历史证据。相关性负结果工件独立提交为 `6bfd4b7`。代码、设计和验收记录分开提交。

## 2. C7–C12 的逐项证据

`tools/verify-core-brain-closure.ps1 -Wave B` 逐个运行 48 个明确命名的测试，并验证每次恰好 1 个通过；测试不存在或变为零测试时，门禁失败。下表的 Gateway 测试同时覆盖 C8/C9，不重复计算为两个测试。

| 场景 | 可执行入口 | 核心正反断言 |
|---|---|---|
| C7 持久三相 tick | cognition `v1_b0_stable_replay` 5 项、store `v1_b1_tick_ledger` 8 项、cognition `v1_b1_tick_rules` 4 项 / `v1_b1_tick_recovery` 4 项、harness `v1_core_brain_b1_tick` 6 项 | 三相有序、同范围/快照、重复 signal 零重复履约、前台优先且 defer 不消费输入、无变化零模型调用；失效和候选之间崩溃可补齐；旧 lease/nonce/attempt/schema/range 不能完成新 claim；unknown 不重试；稳定认知逐字段 replay 相等 |
| C8 AskToLearn | protocol `v1_learning_contract` 1 项、cognition `v1_b2_learning_rules` 2 项、harness `v1_core_brain_b_learning` 4 项、gateway `gatewayd_e2e` 的 C8/C9 进程场景 | 只问影响目标的 scoped gap；L1/Hitchhike；已知、无关、低价值或 quiet 时不发；回答只到低置信 User/CognitiveMap candidate，不晋升；拒绝抑制、延后持久 intention、重启不重复；伪身份零写入；正式 context 搭车装入待问项，回答后移除 |
| C9 全局注意力 | cognition `v1_b3_attention` 3 项、harness `v1_core_brain_b_workspace` 1 项、同一 gateway C8/C9 场景 | goal/run/impulse/intention/candidate/workspace-review 六类；跨 run/workspace 汇总；scope 在容量竞争前过滤；稳定排序与硬容量；digest 绑定全部 cursor、scope、capacity、时间、配置；未来 intention 不入 due 面板；终态项移除；SQLite 和真实 daemon 重启结果一致 |
| C10 有界激活 | memory `v1_b4_activation` 3 项、harness activation 的 C10 场景 | 五种 seed shape、汇聚、衰减、tier/recency/resting；hop/frontier/node/threshold 上限；无 seed 不读图快照/不调模型；不可信或越 scope 不扩散；图 delta 前值/schema/provenance/CAS 校验；在线与 replay 相等；tick 不以自己输出继续自激 |
| C11 直接模型基线 | harness `v1_core_brain_b_activation` 的两项 C11 场景、`forme-core-brain-b-artifact-verify` | 同图/任务/候选/K 与独立标签；实际 profile、usage、延迟和费用；越界模型输出失败；未完成调用不盲重试；篡改指标或错误推荐失败；正式 tick 可使用直接模型且无隐藏 fallback |
| C12 维护与重评 | harness `v1_core_brain_b_maintenance` 3 项 | evidence age、long-unseen、可信 Contradicts 与低价值分支四类；failure → reevaluation → downgrade candidate；根及传递派生失效；confidence 不提高，不删历史；旧图不衰减新知识，非图 append 可安全 rebase 后 CAS；同窗口不重复维护；超过 64 个健康对象不能挡住后面的真实问题 |

Gateway 场景 `c8_c9_learning_http_control_is_authenticated_scoped_durable_and_candidate_only` 使用真实 `forme-gatewayd` 子进程、回环 HTTP 和同一 SQLite：验证未认证拒绝、认证/CSRF、声明命令幂等、显式 tick、提问事件、owner 回答、candidate-only、投影读取、进程重启以及回答重提幂等。该夹具关闭自动 scheduler，以便固定显式 tick 的因果顺序；正式 scheduler 共用运行时的回归由 A/C7 场景证明。

## 3. 相关性工件与负结果

工件：`core-brain-b-artifacts/relevance-comparison.json`；说明和生成方法见同目录 README。

- 证据等级：`ControlledHttp`，标签来源 `IndependentFixture`，不是线上 provider 测评。
- 同一候选集 `other/topic/useful`，独立正确标签 `useful`，K=1。
- 扩散选择 `topic`，precision/recall 均为 0，错误数 1。
- 直接模型选择 `useful`，precision/recall 均为 10000 bps，错误数 0；实际绑定 `core-b-provider:core-b-model`，usage 为 20 input / 4 output，按配置费率记 1 microunit。
- 推荐 `DirectModel`；模型自身选中什么不作为质量真值。此用例证明负结果能影响推荐，不能外推为某种策略在所有任务上更优。
- 工件 digest：`sha256:eb17b673d07cd52667ef49277631d51472b31da71ae5af22b904362698bdd303`。

`CognitiveRuntimeConfig.relevance_mode` / `FORME_RELEVANCE_MODE=direct-model|diffusion` 已进入正式 tick；默认直接模型。无 seed 时两条路径都不扫描/不调用模型。直接模型输出只允许候选集合内至多 K 个唯一 ID；失败或未完成调用保留实际失败/未知证据，不用 fallback 冒充原 profile。

## 4. 回归与交付门

本波实现树的专项结果：

| 命令 | 结果 / 证据 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test --workspace --all-targets` | PASS：493 通过、0 失败、4 默认 ignored；真实 golden 由 M5 门显式运行 |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `bash tools/compliance-doctor.sh` | PASS：upstream/license/copy |
| `tools/verify-core-brain-b1.ps1` | PASS：22 项 B1 模块场景 |
| `tools/verify-core-brain-closure.ps1 -Wave A` | PASS：43 项按名场景，A 波未回退 |
| `tools/verify-core-brain-closure.ps1 -Wave B` | PASS：48 项按名场景及仓库 C11 工件独立验证 |
| `tools/verify-core-brain-closure.ps1`（Final） | 预期拒绝：C/D 尚无实施清单，不允许提前闭合 |
| `git diff --check` | PASS |

整波还必须在隔离的干净检出执行 `tools/verify-m5.ps1`：S1–S99、显式真实 Browser/三进程 golden、历史 M4 receipt、所有 Rust/Python 工程门、合规/漏洞/供应链与当前树 M5 release audit。此检出只含已提交的 B 波内容，不将工作目录中原有的六个许可证修改混入验收。

首次 clean-tree audit 实际阻断了 `rustls 0.23.45` 的 `RUSTSEC-2026-0346/0347/0348/0349`（2026-10-14 公告），其他七组审计通过。修复采用同一 minor line 的 `0.23.46` 精确 pin，先登记依赖文档，再同步 execution manifest、lockfile 和冻结依赖门；不添加 advisory ignore，不放宽审计，也不改变内部 crate 图或 TLS 权限。最终回归必须包含该补丁。

当前树 M5 receipt 位于 `m5-release-audit-artifacts/`，按既有流程在其他代码/文档提交之后独立刷新，再运行完整 M5 门并对比生成 receipt 与仓库 receipt。只有审计 clean-tree、typed receipt 独立验证和当前树比对同时通过才构成发布证据；专项 PASS 或历史 receipt 单独均不能替代。当前 receipt 是本报告之后的独立提交，不在此写自引用提交号或 digest。

## 5. 冻结契约与运行配置

- 18 crates、内部依赖边不变；99 EventKinds，前 97 项为 M5 exact prefix，仅既定两个 R 事件处于末尾。本波未增加 EventKind。
- 冻结 trait 不重设计；扩展通过 companion 和既有载荷的 optional additive 字段。旧载荷缺省省略新增字段，历史字节和工件兼容有专项验证。
- 稳定状态仍经 store 单写者；graph delta 使用封闭、有界的 content-addressed 载荷，事务内校验 expected head 与前值；所有重评/候选/图维护可 replay。
- tick 默认启用、间隔 60000ms；`FORME_COGNITIVE_TICK_ENABLED` / `FORME_COGNITIVE_TICK_MS` 可配置。`FORME_STABLE_EVIDENCE_MAX_AGE_MS` 默认 30 天，只决定生成重评候选的时机。
- owner 学习控制 API：`POST /v1/brain/learning/goals`、`/answer`、`/resolve`；注意力读取：`GET /v1/brain/workspace`。全部经正式 Gateway 身份边界与 ProductionRuntime owner 复查。

## 6. 明确边界

本波不是 M6，没有启动 C 波 C13–C18 的 charter/八套编排/长期项目重协调，也没有启动 D 波 C19–C24 的多模型深化、dashboard、备份/加密/真实删除或最终长期价值 golden。WorkspaceReview 在 B 波只作为到期注意力项，不替代 C 波项目监管。

学习候选不等于正确知识，维护触发不等于已完成重评，受控 HTTP 对照不等于线上长期效果。工程自检与证据不替代 owner 验收和合入 main；历史 B1 报告仍只描述当时的模块结果。
