# M3-C 长期认知、信任与伙伴策略验收报告

- 验收日期：2026-07-19
- 验收范围：M3-C S63-S69，并永久回归 S1-S62
- 工程规模：18 crates；89 EventKinds；311 个 listed Rust tests（309 个常规 + 2 个真实浏览器 ignored tests 由统一门单独实跑）；19 个 Python gate tests
- 权威规格：`docs/requirements/10-m3-verification-strategy.md` S63-S69；`docs/prd/20-m3-program-prd.md` §5
- 最终结果：**PASS - M3-C 与 M3 final gate 全绿**

## 1. S63-S69 逐场景证据

| 场景 | 可执行证据 | 事件或状态序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S63 StrategyMemory | `forme-memory::s63_conflict_retraction_and_decay_produce_governed_recommendations`、`s63_untrusted_content_never_creates_lineage_or_maintenance_actions`、`s63_append_only_resolution_keeps_history_and_removes_active_v2`、`s63_malformed_control_payloads_and_candidate_envelopes_fail_closed` | 冲突/失效链为 `CandidateCreated* -> CandidateConflictDetected -> CandidatePromoted -> StrategyActivated -> RetractionEvent`，投影产生 `Reevaluate -> Downgrade -> Rollback` 建议；落地链为 `RetractionEvent -> ReevaluationTaskCreated -> CandidateDowngraded -> StrategyRolledBack`，最终 v2 inactive、历史仍在 | Untrusted candidate 可见但 `trusted_lineage=false`、零结构边、零维护建议；candidate id/envelope 不一致、错误 aggregate committed version、invalid rollback 全部 fail closed；无 last-write-wins、静默删除或 breaking migration | PASS |
| S64 AgentSelf | `forme-cognition::s64_agent_self_uses_result_evidence_and_self_assessment_only_lowers`，并回归 M3-A evaluation/promotion 链 | 状态变换为 verified pass ×2 得到结果 ceiling=10000；高自评不抬高，低自评只把 effective ceiling 压至 3000；pass+fail 的结果 ceiling=4000；单次 failure 为 `NeedMoreEvidence`。稳定化仍须 `CapabilityEvidence/FailureEvidence -> CandidateCreated -> EvolutionEvaluationRecorded -> CandidatePromoted|Downgraded` | 一次成功不形成全局能力；self assessment 不能覆盖 verified failure；一次失败不永久否定；AgentSelf 不能产生 permission、trust 或 grant | PASS |
| S65 Partnership | `forme-cognition::s65_partnership_requires_process_timepoints_and_authenticated_owner_correction`，并回归 M3-A owner/evaluation gate | 两个过程时间点（verified collaboration + authenticated owner correction）才得到 `PromoteCandidate`；短期例外 + external input 保持 `NeedMoreEvidence`；stable 链仍为 `CandidateCreated -> EvolutionEvaluationRecorded -> CandidatePromoted|Rejected|Downgraded` | external participant 伪造 owner correction 直接报错；短期例外不稳定化；固定 identity、真实性、owner representation 与授权不在可变 spec 中 | PASS |
| S66 Trust/Delegation | `forme-cognition::s66_trust_success_stops_at_owner_proposal_and_failure_downgrades` | verified success ×3 只到 `OwnerReviewProposal`；空 principal 拒绝，authenticated owner review 后 ceiling 仍只为 `RequestNarrowGrant`；单次 verified failure 立即为 `DowngradeAutomatically`。策略事件与既有 grant lifecycle 分离 | success 不自动 promotion/activation/grant；failure 可收紧但不能扩大；无 self-model 决定 delegation、standing L5、撤销后继续执行或 trust 覆盖 permission | PASS |
| S67 Proactivity/Communication | `forme-cognition::s67_proactivity_reduces_regret_without_missing_commitment_or_bypassing_attention`、`s67_communication_strategy_never_expands_recipient_disclosure_or_outward_authority` | candidate 降低 reject/regret/cost 且不漏 commitment 时为 `PromoteCandidate`；漏一个 commitment 即 `DowngradeAutomatically`。AttentionBudget 不足使 commitment 降到 Internal，interrupt cap 使 Interrupt 降到 Digest。communication 状态为 unauthorized surface=`Refuse`，outward 未批=`NeedActionApproval`，批准后=`PrepareOnAuthorizedSurface`，Disclosure Refuse 永远=`Refuse` | 不能为降打扰漏确定承诺；不能突破 AttentionBudget；策略不能新增 recipient、surface、disclosure、observation scope 或 outward authority；external feedback 仍只是 context | PASS |
| S68 真实 Browser 进化 Golden | ignored real test `forme-harness::s68_m3_candidate_live_v2_regression_rollback_and_live_v1_are_governed`；`forme-eval` 的 S68 artifact 正/反四项；`forme-m3-c-artifact-verify` | 控制链精确为 `CandidateCreated -> VerificationStarted -> VerificationFinished -> EvolutionEvaluationRecorded{Pass} -> CandidatePromoted -> StrategyActivated{v1->v2} -> SessionBound{v2} -> M2 governed action -> FailureEvidenceRecorded -> VerificationStarted -> VerificationFinished -> EvolutionEvaluationRecorded{Fail} -> StrategyRolledBack{v2->v1} -> SessionBound{v1} -> M2 governed action` | v2/v1 两次 live action 都含 approval、执行前 recheck、CompetenceGate、ActionBackend、Untrusted receipt、verification；缺 phase、隐藏 regression、最终 active=v2、secret/private path、语义篡改或额外目录项均拒绝 | PASS |
| S69 Release audit | `py -3 tools/release_audit.py --verify docs/acceptance/m3-release-audit-artifacts`；统一门重生成、verify、compare；`tools/tests/test_release_audit.py`；S22 fixtures + `bash tools/compliance-doctor.sh` | typed checks 固定为 `release_tree -> license_notice -> direct_dependencies -> borrowing_records -> upstream_copy -> acceptance_artifacts -> vulnerability_audit`，7 项均 `outcome=pass, blocking=false`，每项保存 tool/version/input digest；copy input 同时绑定 release tree 与 upstream corpus | dirty tree、LICENSE/NOTICE 缺失、依赖未精确 pin/未登记、borrowing 漂移、复制命中、upstream corpus 缺失、secret/private path、artifact 篡改、扫描失败或当前 target 可达 advisory 均 blocking；不能降级为 warning | PASS |

S1-S62 不是抽样回归：`tools/verify-m3.ps1` 先完整调用 `tools/verify-m2.ps1`，再逐项运行 M3-A/B/C 精确场景。因此 M0-M2 的真实 Browser/API/communication、M1 real-model artifacts、M2 typed artifacts、M3-A 五件套、M3-B long-horizon artifact、MCP race regression、CAS/rollback 与原创门全部属于本波完成证据。

## 2. 冻结契约与治理边界

- `EventKind::ALL.len() == 89`；M3-C 没有新增、重排、改名任何事件。前 86 项保持 M2 snapshot，M3 只保留 A 波 additive 的三项。
- workspace 保持 18 crates，内部依赖边无新增、无反向；`EventStore`、`VersionedEventStore`、Loop、Coordination、ActionBackend 和 Harness 入口签名未改。
- M3-C 六类 domain payload 均 versioned、scoped、content-addressed、strict decode；registry 要求六个 seed 完整存在，同一 version/different digest 拒绝。
- StrategyMemory 从事件重建 candidate/stable/active/evidence/derived edges；Untrusted 内容不创建可信 lineage。retraction、decay 与 conflict 只产治理建议或追加控制事实，不重写历史。
- AgentSelf、Partnership、Trust/Delegation 以 verified result 和 authenticated owner correction 为上限；自评、外部反馈、短期例外只能压低或保持未决。
- Proactivity/Communication strategy 只能调整阈值、成本、摘要与当前已授权 surface 偏好；AttentionBudget、Commitment、ObservationScope、recipient、DisclosurePolicy、L3/L5 和 permission 不可变宽。
- candidate、stable、active、permission/grant 四态继续分离。M3-C domain engine 没有 event writer、grant writer 或 active pointer；写入仍经 Harness + store CAS。
- 真实 Browser run 没有实验豁免：两次受控 live run 都经过 M2 的 owner approval、plan digest、执行前重查、CompetenceGate、Untrusted provenance、ground-truth verification 和 unknown-outcome no-retry 边界。
- `LICENSE`、`NOTICE`、直接依赖表和 borrowing records 已进入 release audit；M3-C 未新增 runtime 第三方依赖。
- linked worktree 通过 `.git/commondir` 使用主 worktree 的 `research/upstreams/`；语料缺失或 scanner 异常均阻断。常见 Rust `derive` 只从长表达误报规则中排除，exact/normalized/token/naming/directory 检查保持启用。
- clean-tree 摘要绑定 Git index 的 mode + blob identity，不读取 checkout filter 后的 tracked 文件字节；同一 Git tree 在 LF/CRLF worktree 上必须得到相同 receipt。dirty status 仍在摘要和 blocking finding 中保留。

## 3. S68 Governed Golden

仓库工件：`docs/acceptance/m3-c-artifacts/governed-golden-b4f141e5d930f47e.json`

| 证据 | 值 |
|---|---|
| content receipt | `fnv64:b4f141e5d930f47e` |
| SHA-256 文件校验 | `f949848e8421e96a19dfa3397a716a5cba27725f4eb286233549bf225a4466ce` |
| baseline live v1 | `run:1784384901208560300:1`；35 events；真实 mutation ordinal 1 |
| activated live v2 | `run:1784384905138103100:1`；36 events；真实 mutation ordinal 2 |
| rollback 后 live v1 | `run:1784384909174799600:2`；36 events；真实 mutation ordinal 3 |
| portable lineage | 118 typed events；按 `event_order` 封闭排序 |
| active transition | `loop:m3-a-browser:v1 -> v2 -> v1`；aggregate version `1 -> 2 -> 3` |
| 最终 active | `loop:m3-a-browser:v1` |
| rollback 事实 | trigger=`evidence:m3-a-browser-regression-injected`；`external_effects_reverted=false` |

三次 mutation 都由本机已安装 Chrome/Edge 对 repository-owned loopback fixture 实际完成。baseline、v2 和 restored v1 各自捕获真实 server mutation；promotion/rollback 没有重放已有副作用。artifact 只保存 typed refs/digests、EventKind、snapshot 和 ground truth ordinal，不保存 URL、raw DOM/body、credential、SecretRef identifier 或 owner 私有路径。

`forme-m3-c-artifact-verify` 对唯一 regular file 执行 schema/content address、baseline/eval/control/live/rollback lineage、最终 v1、mutation ordinal、event order、sensitive marker 和封闭目录检查。统一门既验证仓库工件，也重新生成临时真实 Browser 工件后独立验证。

## 4. S69 Release Audit

仓库工件：`docs/acceptance/m3-release-audit-artifacts/release-audit-fc88e2184e29da666c5fc6373ee08d9829ae127899b901ddbd413364e5ba2c3e.json`

| 证据 | 值 |
|---|---|
| typed receipt | `sha256:fc88e2184e29da666c5fc6373ee08d9829ae127899b901ddbd413364e5ba2c3e` |
| SHA-256 文件校验 | `629ec7dcf3fa4c68c61b4167a3e3f631f58b287ba0c6a802c159657a6d708b92` |
| release root digest | `sha256:92b83e18617c4631582216671393c554c6314133f65f02c784464b7d07ecae49` |
| upstream copy input digest | `sha256:bcf349e017654e36645bed6c1ba847264c94404872e693a3862da96e10be6e91`（绑定 release tree + upstream corpus） |
| release audit version | `forme-release-audit/3` |
| scanner | `cargo-audit 0.22.2` |
| release target | `x86_64-pc-windows-gnu` |
| aggregate | `outcome=pass`；`blocking=false` |

`cargo-audit` 报告 `RUSTSEC-2026-0194` 与 `RUSTSEC-2026-0195` 命中锁文件中的 `quick-xml 0.30.0/0.39.4`。审计器没有 ignore 它们，而是逐项执行 `cargo tree --locked --target x86_64-pc-windows-gnu -i package@version`；四项在当前 release target 均不可达，因此作为 typed `TARGET_UNREACHABLE_ADVISORY` observations 保留。任何当前 target 可达 advisory 仍为 blocking finding。

release root digest 排除本 release artifact 目录以及两份自引用最终报告，避免“报告描述自身”导致循环摘要；其余 tracked release tree、dirty status、依赖/许可证/NOTICE、borrowing、原创扫描和全部 acceptance artifacts 都纳入检查。统一门从干净树重生成临时 artifact，并与仓库 receipt 做精确 digest 比较。

封板复验在 `main` 首次暴露一个真实门禁缺陷：linked worktree 因本地 ignored upstream corpus 不在其目录中而曾静默跳过复制扫描；合并到主 worktree 后，扫描又把通用 Rust `derive` 样板判成 `LONG_EXPRESSION_COPY`。修复后 linked worktree 必须解析 shared corpus，缺失时 `UPSTREAM_CORPUS_MISSING` fail closed；仅长表达规则排除通用 `derive`，其余复制检查不变。新增四项 fixture 后 Python gate 从 14 增至 18。

第二次 `main` 复验又暴露 receipt 对 checkout 换行敏感：同一 Git blob 在热修 worktree 保持 LF、合并 checkout 变为 CRLF，旧 audit 因读取工作树字节而产生不同摘要。v3 改为绑定 Git index canonical identity，并新增真实 linked-worktree fixture，断言两边物理字节不同、Git tree clean、7 项 input digest 与最终 artifact digest 相同；Python gate 因此增至 19。

## 5. 最终门

统一入口：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m3.ps1
```

该入口顺序执行：

```text
S1-S52 permanent regression via tools/verify-m2.ps1
S53-S68 exact positive and negative scenario contracts
89-kind protocol compatibility and legacy decode
18-crate graph and exact dependency pins
M3-A real browser artifact regeneration + repository verification
M3-B long-horizon artifact regeneration + repository verification
M3-C real browser evolution regeneration + repository verification
cargo fmt/check/strict clippy/workspace tests
19 Python gate tests
originality compliance doctor + git diff --check
S69 clean-tree release audit regeneration/verification/current comparison
```

最终终行：

```text
M3 FINAL ACCEPTANCE: PASS (S1-S69 + 89 EventKinds + 18 crates + governed evolution golden + release compliance)
```

## 6. 残余边界

- S68 使用真实本机 Chrome/Edge 和真实 loopback mutation，证明受治理外部副作用，不宣称公网、多租户、remote executor 或通用 browser intelligence。
- M3 strategy rollback 只恢复后续 run 的策略选择；`external_effects_reverted=false` 明确保留，外部补救仍须新建受治理 ActionIntent。
- FNV64 是仓库既有 deterministic content-address receipt，不是密码学承诺；报告另列 SHA-256 文件校验，release audit 自身使用 SHA-256。
- 两个 quick-xml advisory 当前仅在 Windows release target 外可达；未来若 release target 或依赖 feature 改变，统一门会重新计算 reachability，不能沿用本次结论。
- release audit 是工程合规证据，不替代法律意见，也不执行公开发布、push 或 merge。

## 7. 结论

M3-C 的 StrategyMemory、AgentSelf、Partnership、Trust/Delegation、Proactivity/Communication 与真实 Browser evolution/rollback 已通过同一套 M3-A 控制面工作；没有新增授权通道、事实源或治理旁路。S1-S69、89 EventKinds、18-crate 冻结图、真实受治理 golden、typed artifacts、strict Clippy、原创门和 clean-tree release audit 均满足。**M3-C 验收 PASS，M3 实现进入可交付状态。**
