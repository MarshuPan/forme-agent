# V1 核心大脑闭合 A 波（产品默认装配）验收报告

- 日期：2026-09-27
- 分支：`shu/v1-core-brain-a`（基线 `main` 94fe6f1；A 波既有 19 个提交之上另有本轮未提交改动，待 owner 审阅后按 prd/23 §12 分提交）
- 范围：C1–C6（`requirements/16` §4），并永久回归 S1–S99
- 权威规格：`requirements/15-16`、`architecture/canonical-contract.md` §28、`architecture/09` §2–4/§12、`prd/23` §1.4 与 §3 A1–A8
- 结果：**自检 PASS，待 owner 验收**。本分支尚未合入 `main`；按 prd/23 §1.4 状态文档纪律，本报告不写“完成/闭合/永久基线”。

## 1. C1–C6 逐场景证据

| 场景 | 可执行证据（`tools/verify-core-brain-closure.ps1 -Wave A` 逐条按名运行并校验“恰好 1 个通过”） | 正向断言 | 反向 / 重启 / 并发断言 | 自检 |
|---|---|---|---|---|
| C1 唯一 composition root | harness `production_builder_composes_one_non_degraded_brain_and_formal_context`、`production_builder_fences_wrong_owner_and_second_live_writer`、`idle_runtime_heartbeat_keeps_the_sole_writer_lease_and_drop_releases_it`、`c1_startup_recovery_turns_a_crashed_action_into_one_unknown_without_driver_calls`；store `v1_brain_authority` 2 项；cli `cli_defaults_to_the_gateway_daemon_and_shares_its_single_runtime` | builder 装配 memory/cognition/coordination/orchestration/proactivity/scheduler/cognitive tick/各 registry/verifier/federation/ecosystem，doctor 对这 19 项服务逐一给出明确状态且无 Blocking；嵌入式 CLI 与 gatewayd 对同一配置得到**同一 manifest digest** | 不同 owner、同进程/跨进程第二写者均 fail closed；空闲超过 TTL 仍持有租约（心跳），析构即释放；`ActionStarted` 崩溃窗口重启两次恰好 1 个 `ActionOutcomeUnknown`，零模型/driver 调用；内存 store 拒绝承载正式大脑 | PASS |
| C2 跨重启连续 | harness `production_brain_authority_and_manifest_survive_restart_without_synthetic_facts`、`c2_owner_brain_state_continues_across_restart_without_synthetic_facts`、`c2_run_command_idempotency_is_durable_concurrent_and_conflict_safe`；store 确定性/legacy/并发冲突 3 项 | aggregate refs 与 manifest digest 重启不变；memory 事实、CognitiveMap candidate、owner 调度与撤回的 intention 重启后一致；新事实 `stream_seq` 无缝续接；重启前写入的记忆被召回进下一次大脑调用的真实 prompt | 重建不追加伪初始化事实；同键同义返回原回执（含重启后，零写入、零模型调用）；同键异义冲突且零写入；4 线程并发同键只产生 1 个 `RunAccepted`、1 次模型调用 | PASS |
| C3 默认 context | harness `c3_formal_context_carries_history_rules_and_brain_slices_to_the_model`；context 5 项 hydration 单测 | `ContextBuildFinished` 列出实际 sources/slice refs；模型端点实际收到的请求体含上一轮历史与固定规则；大脑级调用含有界记忆 recall 切片 | 冷启动无 History 但仍非空；空 sources fail closed；跨 workspace 请求拒绝；未选 skill 不装正文；外部来源记忆/切片保持 `Untrusted`；非大脑调用不装 recall；凭据只在 HTTP 头，不入 prompt | PASS |
| C4 统一能力装配 | harness 9 项 A6 测试（含 `formal_environment_registers_owner_configured_providers_through_one_lifecycle`）与 `child_route_never_inherits_app_api_connector_authority` | 正式环境路径经 `FORME_CAPABILITY_CONFIG` 装配 MCP（真实 stdio 进程，继承变量送达子进程）、skill、plugin skill、App API；visible toolset 只含 allowlist/已绑定项；多 App API 按 plan 路由 | 无 owner trust/grant 的 MCP/plugin/App API 保持 Disabled；缺继承变量/凭据/schema/驱动为 Unsupported 且不可见；unbound plugin tool 永不可执行；预算不匹配显式拒绝；子代理不继承 App API 连接器授权；未知字段/未来版本/自报信任的配置文档 fail closed 且不回显私有路径 | PASS |
| C5 多入口同语义 | harness `c5_scheduler_pass_runs_through_the_same_runtime_as_owner_turns`；cli `cli_defaults_to_the_gateway_daemon_and_shares_its_single_runtime`、`cli_without_a_configured_gateway_fails_closed_with_guidance`、`cli_question_reaches_http_model_and_prints_the_answer` | owner turn 与 scheduler run 的 `SessionBound` 在 `composition_manifest/policy/model/toolset/workspace` 上完全一致，差异只在 `RunAccepted.source`；CLI 默认经 gatewayd，其 run 的 `SessionBound.composition_manifest` 等于 daemon manifest；scheduler pass 同时驱动到期 intention 与大脑 tick 并遵守节奏 | daemon 运行时嵌入式 CLI 被写者租约拒绝；未配置 gateway 的 CLI fail closed 且不创建 store；默认 CLI 从不打开 authority store | PASS |
| C6 配置失败姿态 | harness `c6_required_configuration_fails_closed_and_optional_providers_degrade`、`production_runtime_approval_consumption_remains_closed_after_restart`、`final_policy_floor_is_rechecked_before_driver_dispatch`、`plan_drift_is_denied_before_driver_dispatch`；approval 2 项；store approval_consumption 2 项；models 3 项 | doctor 报告进入事件；缺 MCP 可执行文件为 Unsupported 行且不暴露 backend；非默认 model profile 缺凭据降级为 Disabled 行、不进入可选集合 | 默认模型缺凭据、无 owner、store 不可写、内存 store 均构建失败；doctor 事件不含凭据值或凭据变量名；审批绑定 approver/plan/action/resources/policy/schema/nonce/expiry，driver 前最终重查与 one-shot 持久 claim，plan 漂移零 driver 调用 | PASS（加密密钥缺失子项见 §5） |

## 2. 本轮发现并修复的承重缺陷

| 缺陷 | 影响 | 修复 |
|---|---|---|
| run 命令幂等只在进程内 map，且检查与写入之间有竞态 | 重启后同键重提会生成没有 `RunAccepted` 的“幽灵 run”并再次调用模型；并发同键可产生两次执行；同键异义不报冲突（R-EVENT） | store 增加只读 `probe_domain_claim`；harness 在首次写入前探测 authority 幂等账本，持锁完成准备，新事件必须以本次生成的 ID 落盘，否则拒绝驱动 |
| 生产 runtime 只在写入时续租 | 空闲或长模型调用超过 TTL 后另一进程可接管，原 daemon 成为被围栏挡住的“僵尸” | harness 持有租约心跳线程（TTL/3 续期，析构时停止并释放） |
| 子代理 governance 依赖全局 `network_allowed=false` 挡 App API，而 A6 已把 App API 网络授权改为按连接器 | 子代理可继承父级 App API 出站授权（治理回退） | 子代理 governance 显式清空 App API 连接器限额；回归测试在撤掉修复时失败 |
| `build_with_store` 接受内存 store | 正式大脑可跑在不持久的临时库上 | 组合根要求 file-backed authority store |
| tick run ID 使用进程内计数 | 重启后新 tick 追加进旧 tick 的事件流 | 组合时从已持久的 tick 流续接编号（B1 以持久 tick claim 替换） |
| 正式入口无法配置 MCP/skill/plugin/App API | C4 只能靠测试 builder 证明 | `FORME_CAPABILITY_CONFIG` 有界 JSON 文档（schema v1，`deny_unknown_fields`，只存凭据变量名，manifest 不可声明信任） |
| 正式 context 的记忆只有“N 条记录”计数 | 大脑记忆实际不进入模型 | 大脑级调用装入有界、按 recency 的 recall 切片，保留节点 provenance |
| 任一 model profile（包括非默认 fallback）绑定失败都会让整个 runtime 构建失败 | 与 A2/C6“必需项 fail closed、可选项 Disabled 并解释”不符 | `ModelRegistry::from_config_degrading`：默认 profile 仍 fail closed，非默认 profile 以 Disabled doctor 行解释并排除出可选集合 |
| CLI 总是在进程内自建 runtime | daemon 运行时 CLI 必被单写者租约拒绝；入口不统一 | CLI 默认经回环 HTTP 调 gatewayd（新增 owner 认证 `GET /v1/runs/{run}/answer`），`--embedded` 显式进程内模式，`--manifest` 输出 manifest digest |

## 3. 冻结契约

- `EventKind::ALL.len() = 99`：前 97 项与 M5 exact prefix 同序同名，仅末尾 `WorkspaceCharterChanged`、`DataLifecycleApplied`。
- `SessionBound.composition_manifest` 为 serde optional additive 字段（`architecture/03` §2.1.1、`architecture/09` §2.4 已先行更新）；`None` 序列化省略，历史 payload 字节不变（`session_bound_composition_manifest_is_optional_additive_and_byte_stable`）。
- 18 crates 与内部依赖边不变；新增依赖仅为已登记且精确锁定的包：harness `serde =1.0.228`、cli（dev）`serde_json =1.0.150`（`tools/verify-core-brain-workspace-contract.ps1`）。
- 冻结 trait 签名未改；`RunGateway` 仅增加带默认实现的 `runtime_manifest_digest`；`ReactiveHarness::from_environment*` 继续委托同一 builder。

## 4. 回归与工程门

最终代码树上的一次完整门禁（2026-09-27）：

- `cargo fmt --all -- --check`：PASS。
- `cargo check --workspace --all-targets`：PASS。
- `cargo test --workspace --all-targets`：PASS，442 通过 / 0 失败 / 4 默认 ignored（真实 Browser 与多进程 golden，由 verify-m5 显式运行）。
- `cargo clippy --workspace --all-targets -- -D warnings`：PASS。
- `bash tools/compliance-doctor.sh`：upstream/license/copy 全部 PASS。
- `tools/verify-core-brain-workspace-contract.ps1`：PASS（冻结 M5 workspace 契约、99-kind 与 97 exact prefix、SessionBound additive 字段、精确依赖锁定）。
- `tools/verify-core-brain-closure.ps1 -Wave A`：PASS，43 个按名场景测试逐条运行且各恰好 1 个通过。
- `tools/verify-core-brain-closure.ps1`（Final）：按设计 **fail closed**——B/C/D 尚无场景清单，不能宣称闭合。
- `tools/verify-m5.ps1`：S1–S99 全部场景门、各阶段严格 Clippy、显式真实 golden 与 M4 历史 receipt 复核 PASS；唯一失败项为 S99 release audit 的 `RELEASE_TREE_DIRTY`（本轮改动尚未提交，release audit 设计上要求干净的已跟踪工作树）。其余 release audit 检查（license/NOTICE、直接依赖、借鉴记录、原创性、验收工件、漏洞审计、生态威胁基线）均 PASS。**提交后须在干净工作树上重跑 `tools/verify-m5.ps1` 作为本波最终回归证据。**

## 5. 残余边界（不在 A 波宣称）

- **C2 workspace charter**：charter aggregate/projection 属 C 波（C13）；A 波的跨重启连续性覆盖 memory、candidate、intention（含 owner 撤回）、aggregate refs、stream_seq 与命令幂等。
- **稳定认知在线/重放不一致（R-FOLD）**：`CognitiveRuntime::govern_and_apply` 晋升时写入调用方提供的 `StableCognition`，而重放从已记录的 proposal 推导（对象 ID、置信度、证据时间、`last_reproduced_at` 均可能不同）。A 波生产路径尚无稳定认知晋升，故未触发；列为 **B 波前置项 B0**，须在三相 tick 的 ReflectAndCandidate 接入生产前修复。
- **C6 加密密钥缺失**：应用层加密属 D 波（C22）；A 波 `DataProtectionConfig` 尚无加密开关，该子断言待 D 波。
- **记忆相关性**：recall 目前按 recency；激活扩散与“直接交模型”基线属 B 波（C10/C11）。
- **大脑 tick**：A 波 scheduler pass 驱动的是既有有界 tick，节奏 claim 为进程内；三相 tick 与持久 tick claim ledger 属 B1。
- **真实模型 golden**：A 波入口证据使用真实 gatewayd 进程与回环 HTTP 模型夹具；真实 provider golden 属最终门（C24）。
