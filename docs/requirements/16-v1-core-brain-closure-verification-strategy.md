# V1 核心大脑闭合验证与验收策略

本文把 `15-v1-core-brain-closure-scope-decisions.md` 的 B1–B24 转成 C1–C24 可执行验收。编号使用 `C`（Closure），明确它不是 S100+ 或 M6；S1–S99 继续作为永久历史回归门。

## 1. 验收原则

1. **默认路径证据优先**：测试必须从正式 CLI/Gateway/environment composition root 进入；只证明手动 builder 能拼出来不算闭合。
2. **事件 + 投影 + 地面真值三证合一**：事件证明发生顺序，投影证明可恢复状态，外部/文件/模型 ground truth 证明结果不是自报。
3. **重启是必测路径**：memory、cognition、charter、AgentWorkspace、intentions、model routing、backup lifecycle 都必须跨进程/重建验证。
4. **正反场景成对**：每个能力同时证明“该发生时发生”和“不该发生时结构上发不出去”。
5. **有用性独立于结构正确性**：C23 使用真实任务与直接模型/简单算法基线；结构全绿但长期价值不优不得宣称“越用越好”。
6. **hard invariant 不可被总分抵消**：越权、密钥泄露、盲重试、稳定层直写、前台被后台抢占、数据删除不完整等任一失败直接阻断。
7. **证据可携带且不含秘密**：acceptance artifact 只保留 digest/ref、脱敏事件和可复验报告，不含 API key、加密 key、SecretRef identifier、私有 endpoint 或本机私有路径。

## 2. 全局回归门

每一波必须先后执行：

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
bash tools/compliance-doctor.sh
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m5.ps1
```

最终门另执行 `tools/verify-core-brain-closure.ps1`，并验证：

- 18 crates 与 `architecture/03 §1` 依赖边完全不变。
- 前 97 个 EventKind 与 M5 exact prefix 同序同名；仅末尾追加 `WorkspaceCharterChanged`、`DataLifecycleApplied`，形成 99-kind taxonomy。
- 永久 M5 回归必须使用历史 97-kind taxonomy snapshot 验证既有 artifact；不得把当前 99-kind `EventKind::ALL` 直接作为 M5 历史工件的等值比较对象。closure artifact 另行验证完整 99-kind。
- S1–S99 与 C1–C24 全绿。
- tracked tree clean；typed artifacts 可离线校验；合规门和 release audit PASS。

## 3. 场景矩阵

| 场景 | 核心证明 | 波次 |
|---|---|---|
| C1 | 正式 composition root 完整装配且单一 | A |
| C2 | owner 大脑 aggregate 跨重启连续 | A |
| C3 | 默认 context 非空且 scope/预算/信任正确 | A |
| C4 | 能力、MCP、skills、plugins、backends 统一注册与 doctor | A |
| C5 | CLI/Gateway/scheduler 使用同一 runtime 语义 | A |
| C6 | 缺配置时 fail closed 或显式降级 | A |
| C7 | 三相 cognitive tick 顺序与前台优先 | B |
| C8 | AskToLearn 从真实认知缺口产生并受治理 | B |
| C9 | owner 级 AgentWorkspace 跨 run/workspace 可重建 | B |
| C10 | 激活扩散有界、可 replay、无 seed 不运行 | B |
| C11 | 激活扩散与直接模型相关性基线诚实对比 | B |
| C12 | 认知老化/冲突/修剪只产事件和候选 | B |
| C13 | WorkspaceCharter event-sourced、CAS、执行约束生效 | C |
| C14 | 八套种子编排完整且 13 模式不平铺 | C |
| C15 | 六维匹配、阈值与定制兜底 | C |
| C16 | route 状态、预算、取消、重试、隔离、冲突上抛 | C |
| C17 | 长期 workspace 被大脑监管并可重协调 | C |
| C18 | route outcome 回流 episode/candidate/fitness | C |
| C19 | 多模型 routing/fallback 与能力适配可审计 | D |
| C20 | 认知/主动 dashboard 只从事件投影 | D |
| C21 | 冷备、完整性检查、恢复与 replay 对账 | D |
| C22 | 应用层加密、脱敏与秘密不落盘 | D |
| C23 | 真实删除/保留清理原文、索引、密钥并留 tombstone | D |
| C24 | 长期价值基线、最终真实 golden 与 release gate | D |

## 4. 逐场景规格

### C1 正式 composition root

- **准备**：仅设置最小 owner/model/store 环境；不调用任何 `with_*` 测试装配方法。
- **动作**：分别创建正式 CLI 与 Local Gateway runtime。
- **必须观察**：model registry、EventStore、EventSourcedMemory、CognitiveRuntime、coordination reasoner、orchestration library、proactivity、scheduler、capability registries、backend registry、eval、federation、ecosystem 都有明确状态；composition manifest digest 相同。
- **反向断言**：正式运行不允许 `NoopProactivity`、`coordination=None`、临时 in-memory store 或空 owner identity；同一进程不能创建两个 authority writer 指向同一 store。

### C2 owner 大脑跨重启连续

- 在第一次进程中写入 memory node、CognitiveMap candidate、intention、workspace charter 和 owner steering。
- 关闭并从同一 SQLite 重新构建 runtime。
- 重启后 aggregate IDs、stream_seq、candidate/intention/charter 状态与 active strategies 必须连续；重复构建不得追加伪初始化事实。
- 修改 owner ID 后打开同一 store 必须 fail closed，不能继承前 owner 的大脑。

### C3 默认 context hydration

- 用真实 run 先产生 session history、memory summary/recall、workspace charter、AgentWorkspace 项、skill metadata 和认知 refs。
- 下一次大脑级模型调用的 `ContextBuildFinished` 必须列出实际来源；装配内容 obey scope、trust treatment 和 token budget。
- 普通执行节点只装必要切片；未选择 skill 不加载正文；Untrusted 内容标 data treatment。
- 反向：`ContextSources::empty()`、跨 workspace memory、raw event dump、超预算静默截断均阻断。

### C4 统一能力与执行装配

- 从 typed config 注册至少 File、Shell、MCP、Skill、Plugin、Browser/Computer/PTY/AppApi 中可在当前环境安全实跑的代表能力；其余形成 disabled/unsupported doctor row。
- registry lifecycle 必须为 discovery -> config -> enable -> trust -> policy -> visible toolset；实际执行仍走 final recheck。
- capability 配置缺失、schema drift、driver 不可用、SecretRef resolver 缺失时 doctor 明确解释且能力不暴露。
- 反向：已安装 package、MCP 返回或 plugin manifest 不能自动获得 trust/permission。

### C5 多入口同一语义

- CLI、Gateway 和 scheduler 对同一输入/同一 scope 使用相同 composition manifest、policy/model/toolset/cognition refs。
- 分别产生 `RunAccepted -> SessionBound -> ContextBuild* -> ...`；差异只能来自 `Source` 与入口 provenance。
- candidate、memory、AgentWorkspace 和 dashboard 在入口之间立即一致。
- 反向：入口私有 memory、入口私有 model loop、入口私有 approval 或绕过 Gateway/Harness 直接执行均失败。

### C6 配置失败姿态

- 构造缺 model credential、无 owner、store 不可写、MCP executable 缺失、encryption key 缺失但敏感加密启用等配置。
- 必需组件缺失则 runtime 构建失败；可选组件缺失则 disabled 且有 doctor explanation，不允许半启用。
- doctor report 进入事件且不包含秘密值。

### C7 三相 cognitive tick

- 准备一个有新 run outcome、失败和待整理 memory 的 owner snapshot，且没有 foreground active session。
- tick 事件序列必须先 observation/opportunity，再 reflection/candidate，再 memory maintenance；报告分别列 phase outcome。
- foreground session active 时，重认知 phase defer；无 diff/到期 intention/新 evidence 时不调用模型。
- 后台不得直接发 ActionStarted、改变 stable cognition 或抢占前台 session lock。

### C8 AskToLearn

- 构造“当前目标需要风险偏好，但 UserModel/CognitiveMap 无该 scope 属性”的真实 blocking/quality gap。
- tick 生成 `ImpulseRaised{Gap}` 和 `ProactiveProposalEmitted{ask-to-learn,L1,Hitchhike}`；问题必须绑定 gap、scope、reason 和候选目标。
- owner 回答后只形成 evidence + User/CognitiveMap candidate；拒绝后 origin 被抑制，延后进入 intention。
- 无关缺口、已知高置信答案、quiet/attention rule 不允许打断式重复提问。

### C9 owner 级 AgentWorkspace

- 同一 owner 创建两个 workspace 的 active goal、一个进行中 run、一个到期 intention、一个待确认 candidate 和多个低价值 impulse。
- 全局投影跨 run 汇总，scope query 只返回允许项；按价值×紧迫排序并硬截断容量；snapshot ref/digest 稳定可重建。
- DecisionTrace 引用决策时 snapshot；重启后相同事件流得到相同排序。
- 反向：当前 run 事件子集不能冒充 owner 全局面板；项目 Workspace 与 AgentWorkspace id 不能混用。

### C10 激活扩散运行语义

- 建立有 association/temporal/contradiction 边的图，按五种 shape seed。
- spread 必须遵守 max_hops、top_k_frontier、surface_threshold、per_tick_nodes；汇聚相加、每跳衰减、tier/recency/resting activation 生效。
- edge weight/巩固/修剪只通过 `MemoryMaintenanceApplied` 变更并可 replay；Untrusted 内容不能改结构/权重。
- 无 seed 返回空且不扫图；超预算确定性截断；相同 snapshot/config 结果稳定。

### C11 直接模型基线

- 对同一候选节点集、相同任务与相同输出 K，分别运行 spreading activation 与“候选集直接交模型选相关项”。
- artifact 记录 case、模型/策略快照、质量、召回、错误、延迟、token/费用和 hard invariant。
- 若扩散在冻结阈值下不优于基线，运行时允许选择基线实现，报告不得把“实现存在”写成“机制更好”。

### C12 认知维护

- 构造稳定认知证据老化、新证据矛盾、长期未复现和低价值图分支。
- tick 产生 FailureEvidence/ReevaluationTask/downgrade candidate 与 MemoryMaintenanceApplied；稳定对象仅被标无效/待重评，不删除历史。
- owner 确认前不得自动提高 confidence、autonomy 或边权；更谨慎的降级可按既有 M3 规则激活。

### C13 WorkspaceCharter

- owner 以 expected-version 创建/更新 charter，内容含目标、约束、禁止项、完成标准/监管 cadence 的声明式引用。
- `WorkspaceCharterChanged` 必须携带 workspace、version、expected/committed version、actor、charter/digest；投影可重建。
- stale version、非 owner mutation、空约束、跨 workspace 引用均零写入。
- context 与 coordination 对违反 charter 的 route/action 必须拒绝或重规划；charter 不能直接改 CognitiveMap。

### C14 八套种子编排

- library 索引逐项暴露八套模式的 purpose、topology、signature、roles、operation guide 和 initial fitness。
- 前四套不 spawn subagent；后四套按模板实例化 route；`long-running-harness` 含 checkpoint/fresh-context 周期。
- 断言 Tool Use/Memory 不作为模式条目，13 种主流模式不被平铺成 13 个同级 enum。

### C15 匹配与定制兜底

- 给八类代表任务生成六维签名，最高分过阈值时命中预期模式，并记录 score/anchors/fitness。
- 所有模式不过阈值时返回 custom route，标 low confidence、加 evaluator/checkpoint，并形成 candidate 而非直接沉淀。
- 模式成功/失败只调整候选 fitness；active pattern policy 仍由 M3 activation 决定。

### C16 route 运行治理

- 对并行、串行、review gate、replan 四类 DAG 验证 Pending/Running/Done/Failed/Cancelled/OutcomeUnknown。
- 子预算从父预算原子预留并回收；父取消级联；同 intent retry 幂等；副作用不明进入 unknown、不得重试。
- 每个子代理只见最小 toolset/scope/model/budget，使用隔离工作目录；结果按 summary/result contract 回传。
- 事实/设计/质量真冲突不能由 integrator 静默裁决，必须保留 conflict 并上抛主脑。

### C17 长期 workspace 监管

- 创建长期 workspace、goal、charter 和初始 direct/no-orchestration route。
- 在新 evidence、失败、deadline/diff 到来时，owner 大脑 tick 重新 frame/plan；可从无编排变为编排、增加/减少节点或换模式。
- 每次变化生成新 DecisionTrace/route/checkpoint，不 mid-run 热换；workspace 本身不创建独立 cognition runtime。

### C18 编排经验回流

- 完成一次成功 route 和一次失败/选错模式 route。
- 两者都形成 episode node/edges，连接 goal/pattern/nodes/result/failure/verification；成功和失败案例锚均保留。
- library `sediment` 只生成 strategy candidate/fitness evidence；不得直接变 active pattern。
- replay 后 episode、anchors、candidate lineage 和 failure digest 一致。

### C19 多模型 routing/fallback

- 配置至少两个真实或可控 scripted provider，能力/窗口/工具/成本/风险不同。
- routing 按任务需求选 model，`SessionBound`/DecisionTrace/ModelCall 记录选型依据与 snapshot。
- 首选出现可重试 transport/provider failure 时，按有界 fallback policy 选择兼容模型；不兼容、L5/high-risk 或结果不明时停止/询问，不无界轮换。
- fallback 不能改变 permission、toolset、scope 或审批；秘密不进事件。

### C20 dashboard

- 由同一 event store 投影 proposal adopt/reject/defer、interrupt regret、AskToLearn usefulness、candidate backlog、map confidence、active workspace supervision、pattern/model outcomes。
- API/CLI 查询支持 owner/global/workspace/time window，结果含 numerator/denominator 和 unverifiable 数量。
- 删除 projection 后可 replay 重建；禁止另建不可追溯统计事实。

### C21 备份、恢复与完整性

- 在有并发读但无未提交 writer 的 checkpoint 制作 SQLite 一致性冷备，生成 manifest（schema、last stream seq、file digest、created_at、owner digest）。
- 篡改一个 byte 后 restore verifier 必须失败；合法备份恢复到新路径后执行 integrity_check、事件 checksum、stream continuity、replay/projection 对账。
- 恢复不能自动取代当前 authority；需要 owner 显式选择目标并保持 single writer。

### C22 应用层加密与脱敏

- sensitive event/object content 用成熟 AEAD 库加密，envelope 只记录 key ref、nonce/ciphertext/version；key 由本地 resolver 获取。
- DB、日志、trace、portable artifact、doctor debug 搜索不到明文、key 和 SecretRef identifier；正常授权读取可解密。
- 错 key、缺 key、tamper 必须 fail closed，不能返回乱码或把密文当普通内容。

### C23 真实删除与保留

- owner 对一个 scoped subject 发起 delete/retention；系统删除/crypto-shred 原始敏感 payload、FTS、summary/vector/graph/candidate 派生内容，并追加不含原文的 `DataLifecycleApplied` tombstone。
- 证明清理前可检索、清理后所有本地索引不可检索；manifest 列出 cleaned projections/destroyed key refs 的安全 digest。
- 审计事件、授权依据和“不宣称远端已删”的事实保留；非 owner、跨 scope、stale expected version 零改变。

### C24 最终价值与真实 golden

- 至少覆盖：简单对话冷启动、复杂工具任务、长期 workspace 监管、多 Agent 复杂任务、主动 AskToLearn/提醒、失败后重协调六类真实任务。
- 每类与直接模型/最简单可用策略对照，报告质量、可靠性、一致性、延迟、成本、打扰和 hard invariant；保留失败 case。
- 真实配置模型、真实 SQLite 重启、至少一个真实外部 backend 和完整 A–D 路径生成封闭 typed artifact 集。
- 最终结论只能是 PASS / FAIL / UNVERIFIABLE；不可用“测试很多”替代价值证据。

## 5. 三类正确性

- **Runtime correctness**：装配、调度、恢复、route、fallback、backup/restore、删除状态机正确。
- **Governance correctness**：owner/auth/scope/policy/approval/competence/candidate/secret/retention 不变量正确。
- **Cognitive usefulness**：memory/context/AskToLearn/activation/orchestration/model routing 对真实任务产生可比较价值，失败能回流。

## 6. Artifact Gate

每波至少产出：

- `docs/acceptance/core-brain-{a|b|c|d}-acceptance-report.md`
- `docs/acceptance/core-brain-{a|b|c|d}-artifacts/manifest.json`
- 该波核心 typed report/trace/ground-truth 文件
- digest、event range、schema/config/model/policy snapshot refs
- secret/private-path scan 结果

最终产出：

- `docs/acceptance/core-brain-closure-acceptance-report.md`
- `docs/acceptance/core-brain-closure-artifacts/`
- `docs/acceptance/core-brain-closure-release-audit-artifacts/`

artifact 必须封闭、确定性、可离线校验；真实 credential 与 endpoint 只在运行环境存在。

## 7. 分波与最终门

1. A 波通过 C1–C6、S1–S99 和全局门后提交 owner review。
2. B 波通过 C7–C12、A/S1–S99 回归后提交 owner review。
3. C 波通过 C13–C18、A/B/S1–S99 回归后提交 owner review。
4. D 波通过 C19–C24、A–C/S1–S99、真实 golden、typed artifacts 和 clean-tree release audit 后，才可声明 V1 核心大脑闭合。

任一波出现协议/依赖图/治理边界变化，先停止实现并回到 requirements/architecture；不得用 acceptance report 为代码偏离补票。
