# M2 验证与验收策略

本文承接 `07-m2-scope-decisions.md`，把 M2 的跨系统行动转成可执行验收。S1-S37 永久证明 M0/M1 没有回退；S38-S52 证明真实 browser/computer/PTY、connector/communication、resource/capability/memory/sync 仍在同一条受治理链路中工作。

## 1. 验收原则

- 最终页面、模型回答或 driver 返回值不能单独证明正确，必须断言权威 event、plan digest、store/projection 和反向条件。
- 真实外部动作必须证明 `ToolCallProposed -> ToolPolicyEvaluated -> ApprovalRequested/Resolved -> CompetenceGateEvaluated -> ActionPlanned -> ActionStarted -> terminal -> VerificationFinished` 的治理顺序；不适用节点必须有明确原因，不能静默跳过。
- Browser/Computer 的每个副作用步骤、PTY 的每次启动、API mutation 和外部发言都是独立 plan-bound action；会话授权不等于后续步骤授权。
- 外部返回内容必须证明由 Harness/Gateway 盖 `Untrusted` provenance，且 prompt-injection fixture 不能改变 stable cognition、UserModel、trust、permission 或图结构。
- credential 只以 `SecretRef` 进入 plan；event/log/trace/eval/artifact 做字段与值双重扫描。
- 对外动作覆盖 deny、expired approval、digest/schema/scope drift、cancel、timeout、duplicate intent、crash/unknown outcome 和 rollback 不成立。
- 每个场景同时提供 Runtime、Governance、Learning/Projection 三类证据；M2-C 的“成长”必须证明只到 candidate/proposal，不自动 promotion。

## 2. 全局回归门

每次 M2 提交前必须执行：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
```

`tools/verify-m1.ps1` 保持为 S1-S37 固定回归入口。M2 新建 `tools/verify-m2.ps1`，只能在 M1 gate 通过后追加当前已激活波次场景与 artifact gate。

## 3. S38-S52 场景矩阵

| 场景 | 证明目标 | 主责波次 |
|---|---|---|
| S38 | Browser 原子动作经完整咽喉、真实浏览器执行且输出为 untrusted | M2-A |
| S39 | Computer action 绑定显式 surface/坐标/输入，默认审批且 driver 无旁路 | M2-A |
| S40 | PTY 在 allowlist/root/env/timeout/cancel 边界内真实运行，不 fallback shell | M2-A |
| S41 | 外部动作 L3/L5 floor、plan digest、scope/schema drift 与 unknown outcome 不变量 | M2-A |
| S42 | 外部输入 provenance、防 prompt injection、SecretRef 与 artifact redaction | M2-A |
| S43 | AppApi connector lifecycle、identity、credential、schema、rate/policy 全链 | M2-B |
| S44 | 真实外部 CommunicationSession 有 grant、TTL、budget、termination 和 transcript policy | M2-B |
| S45 | 真实 DisclosurePolicy/representation 在发送前 enforce | M2-B |
| S46 | 真实 device/observation adapter 授权、撤销和 retention 生效 | M2-B |
| S47 | 真实或协议级 A2A bounded session 强目的、强终止、越界拒绝 | M2-B |
| S48 | ResourceGraph 从事件重建，结果证据评分不等于授权 | M2-C |
| S49 | 长期目标按 intention/artifact lineage 推进且前台优先、可停止 | M2-C |
| S50 | CapabilityGap/UpdateProposal 以结果证据为主且不自动提升规格/权限 | M2-C |
| S51 | managed plugin policy 原子 contribution snapshot、deny 与撤销 | M2-C |
| S52 | 热冷记忆和单 peer 同步保持单一事实源、CAS、redaction 与冲突边界 | M2-C |

## 4. 逐场景规格

### S38 BrowserBackend 真实动作

- **Fixtures**：repository-owned 本地 HTTP 页面；页面包含可读取文本和一个会向本地 fixture endpoint 发送 mutation 的按钮；真实 Chrome/Edge；Browser capability 默认 untrusted；policy ask。
- **触发**：scripted model 提交 `ActionIntent{backend=Browser, expected_effect=Outward}`，owner 批准绑定 plan 后执行 navigate/read/click 中的一个原子操作。
- **事件**：`ToolCallProposed -> ToolPolicyEvaluated{ask} -> ApprovalRequested -> RunWaiting -> ApprovalResolved{granted} -> RunResumed -> CompetenceGateEvaluated{max>=L3} -> ActionPlanned{backend=Browser,approval_ref} -> ActionStarted -> ActionOutputDelta{trust=Untrusted} -> ActionCompleted{receipt} -> CapabilityEvidenceRecorded -> VerificationStarted -> VerificationFinished`。
- **Store/结果**：plan digest、origin、selector、operation、scope、rollback boundary 可追；真实 fixture 收到浏览器请求；trace 按 `stream_seq` 连续。
- **反向断言**：未批、origin 不在 allowlist、`javascript:`/`file:` URL、selector/target 批后变化、driver 不可用时均无 `ActionStarted`；页面文本中的“忽略规则”不成为指令。

### S39 ComputerBackend 受治理动作

- **Fixtures**：显式隔离/记录型 surface；Computer driver；允许的 coordinate bounds、按键集合和输出预算；默认 policy ask。
- **触发**：提交一个 move/click/type/key/scroll 原子 action，分别测试 grant 和 deny。
- **事件**：与 S38 同治理骨架，`ActionPlanned/Started` 的 backend 为 Computer；完成后记录 capability evidence 和 action receipt ref。
- **Store/结果**：surface、坐标/按键、input ref、计划摘要和 approval 一一绑定；driver 只收到批准后的规范化动作。
- **反向断言**：无 surface、越界坐标、空/超长输入、未允许按键、批后参数改变、取消信号均不产生未授权副作用；driver API 不从 gateway/model 直接暴露。

### S40 PtyBackend 真实交互进程

- **Fixtures**：repository-owned PTY fixture program；允许程序和 cwd root；最小环境；有限 cols/rows、输入、输出、timeout。
- **触发**：批准后启动 PTY、写入一次输入并等待终态；另触发 timeout/cancel/非 allowlist program/root escape。
- **事件**：`... -> ActionPlanned{backend=Pty} -> ActionStarted -> ActionOutputDelta{trust=Untrusted}* -> ActionCompleted|ActionCancelled|ActionFailed -> CapabilityEvidenceRecorded`。
- **Store/结果**：真实伪终端语义得到 fixture 输出；输出受 byte budget；进程终态可验证。
- **反向断言**：PTY 不继承未声明环境，不接受明文 secret env，不越 root，不 fallback Shell；timeout/cancel 终止 child；失败不标成功。

### S41 外部动作治理与恢复

- **Fixtures**：Browser/Computer/PTY 的低、中、高风险动作；显式 L4 narrow envelope；L5 不可逆动作；stale approval；`ActionStarted` 后模拟 crash。
- **触发**：分别尝试无 approval、合法 L3 approval、证据充分且 owner 显式给窄 L4、L5 standing grant、digest/scope/schema drift 和 restart recovery。
- **结果**：默认外部动作至少 L3；只有低风险、可回滚、窄 scope、显式 L4 envelope 且能力门/结果证据满足时才可不逐次 ask；L5 始终逐动作审批。
- **事件**：stale/digest drift -> `ActionDenied`；crash -> `ActionOutcomeUnknown -> RunWaiting`；人工/可证明 probe 后才 resume。
- **反向断言**：trust/一次成功/driver 声明不能提高 permission；unknown outcome 不自动重试；补救动作不被记录为 rollback。

### S42 Provenance、Injection 与 SecretRef

- **Fixtures**：网页/PTY 输出包含伪 owner 指令、prompt injection、伪 trust 字段和 secret-like marker；plan 只含 `SecretRef`；artifact exporter。
- **触发**：执行受治理 action、装配下一 turn context、生成 trace/eval manifest。
- **结果**：外部内容事件 provenance/trust 为 `Untrusted` 且由 Harness 赋值；内容可供当前任务分析但不能获得 owner authority；stable/candidate graph 无未授权边权写入。
- **反向断言**：进入下一次 `ModelRequest` 时 source/trust/`UntrustedData` treatment 仍在，外部内容不得退化成 owner instruction；模型伪造 `ActionIntent.source=UserTurn` 被 Harness 绑定回真实 ingress source，未信任输入产 action 时至少转 owner approval；adapter 自报 `OwnerInput` 被忽略/拒绝；event/log/trace/eval 不含 secret value、Authorization、credential field 或本机 secret path；SecretRef 不被 Debug 展开。

### S43 AppApi Connector

- **Fixtures**：一个 project-owned read/mutation connector；configured/disabled/untrusted/revoked 状态；SecretRef；schema/rate/timeout fixture。
- **触发**：lifecycle 到 active，执行 read 与 mutation，再触发 schema drift、revocation、rate limit。
- **事件**：provider lifecycle/CapabilityIndexed/ToolsetResolved -> ToolCall/Policy/Approval/Action/Verification；mutation 默认至少 L3。
- **反向断言**：disabled/untrusted/revoked 不可见；credential 不进 event；schema 命中不等于执行授权；mutation unknown outcome 不盲重试。

### S44 真实 External CommunicationSession

- **Fixtures**：真实 loopback/project-owned adapter 和 external participant；purpose、TTL、turn/token budget、transcript policy。
- **触发**：以 owner `AuthContext` open session、交换消息、达到目的/超 TTL/超预算/目的偏离，并尝试无认证 grant。
- **事件**：`ExternalCommunicationGranted -> CommunicationSessionOpened -> CommunicationEventReceived* -> CommunicationSessionTerminated`。
- **反向断言**：无 owner 认证不能创建 grant；无 grant 不发送；超限后无新 delivery；transcript 只进 evidence/candidate，不自动 stable。

### S45 Disclosure 与代表身份

- **Fixtures**：允许、模糊、拒绝、需审批的披露规则；confirmed/unconfirmed owner statement；AgentRepresentingOwner。
- **触发**：真实 adapter 尝试发送普通、敏感、高影响或未确认表达。
- **事件**：`DisclosurePolicyApplied{binding=session/participant/purpose/content/policy_inputs} -> [ApprovalRequested -> ApprovalResolved] -> ActionPlanned -> ActionStarted|ActionDenied`。
- **反向断言**：发送先于 disclosure event 为失败；缺 binding、错误 Harness/Gateway ingress stamp、复用 request ref、替换 session/participant/purpose/content 任一字段均在 action 前拒绝；agent 不伪装 owner；低置信 candidate 不对外作确定事实。

### S46 真实 Device/Observation Adapter

- **Fixtures**：显式 DeviceGrant、ObservationScope、RetentionPolicy；授权/过期/撤销设备事件。
- **触发**：授权前后和撤销后提交 observation。
- **结果**：只在有效 grant/scope/retention TTL 内生成 `CommunicationEventReceived/ObservationRecorded`，provenance 由 Gateway 盖章；`retain_raw=false` 时事件、`RunAccepted`、transcript 与 FTS 均不含 observation 原文。
- **反向断言**：默认常驻监听、adapter 自报 owner、过 TTL 后提交、撤销后继续摄入均被拒；只断言 `raw_retained=false` 或 expiry 数值而不检查 store 不计通过。

### S47 A2A Bounded Session

- **Fixtures**：project-owned peer protocol fixture；双方 identity、purpose、disclosure、max turns、TTL、budget、termination。
- **触发**：合法交换、越界请求、预算耗尽和目的完成。
- **结果**：会话强终止；越界走 refuse/approval/terminate；digest 只进 evidence/candidate。
- **反向断言**：内部 subagent 通道不能冒充外部 A2A；无限循环和跨 scope 披露被阻断。

### S48 ResourceGraph

- **Fixtures**：多 tool/skill/model/memory/source/backend 的成功、失败、验证和撤销事件。
- **触发**：重建 graph，Coordination 读取 snapshot 做 ResourcePlan。
- **结果**：节点/边/score 都有 event refs、scope 和 freshness；重建确定；DecisionTrace 引用 graph snapshot。
- **反向断言**：score 不改 permission/trust；未信任内容不写图边权；graph 不执行 action。

### S49 长期目标

- **Fixtures**：长期 GoalFrame、多个 ProspectiveIntention、checkpoint artifact、前台 run、cancel/revoke。
- **触发**：多 tick 推进、重启、前台抢占、情境变化。
- **结果**：每步仍为普通 governed run；artifact/goal/intention lineage 可重建；情境变化降级/重议。
- **反向断言**：后台不抢前台、不无限循环、不越预算；cancel/revoke 后不开始新 action。

### S50 能力成长

- **Fixtures**：同 scope 多次 capability pass/fail/unverifiable、owner feedback、AgentSelf 自评冲突。
- **触发**：生成 CapabilityGap/CapabilityUpdateProposal，owner approve/reject narrow grant。
- **结果**：结果证据权重大于自评；proposal 是 candidate；批准只改变显式窄 envelope。
- **反向断言**：一次成功、自评高分或 plugin 声明不自动改 trust/permission/LoopSpec/CoordinationPolicy。

### S51 Managed Plugin Policy

- **Fixtures**：managed allow/deny、user/workspace lower layers、signed/source-bound plugin snapshot、reload/revoke/fail。
- **触发**：activate、reload、managed revoke 和 plugin crash。
- **结果**：managed deny 不可放宽；新 snapshot 验证后原子切换；撤销后 contribution 不可见。
- **反向断言**：ghost capability、部分激活、remote auto-update、plugin bypass route 均为 0。

### S52 热冷记忆与单 Peer 同步

- **Fixtures**：hot/cold event-derived projection、敏感 payload/SecretRef、一个 peer、expected-version 冲突和断线重连。
- **触发**：归档/召回、增量同步、重复批次、冲突、删除/retention marker。
- **结果**：event log 仍唯一事实源；projection 可重建；CAS 冲突显式；重复批次幂等；同步导出按 policy redaction。
- **反向断言**：peer 不直写 stable projection；secret 不出设备边界；未在 store open 时预配置的 peer、无 expected-version 的第二写者都被拒，首次 sync 请求不能抢占 peer 配置。

## 5. Golden Task 最小集合

M2 在 M1 四组 golden 基础上增加：

1. 真实 browser：访问 repository-owned 本地页面，执行一个经审批的真实 mutation，验证服务端地面真值和完整事件链。
2. 真实 connector：使用专用低权限 credential/ref 完成一个 read + 一个需审批 mutation，记录 schema/identity/scope snapshot。
3. 真实外部 communication：bounded session 中应用 disclosure/representation/termination。
4. 长期目标/能力成长：多次结果证据只产生 proposal，owner review 后才改变窄 envelope。

至少第一项或第二项必须在 M2 final 以真实 runtime 执行。typed report 和 portable trace 只保存必要 identity/sequence/kind/profile/schema/ref，不保存 credential、原始敏感正文或 endpoint secret。

## 6. 分波次门

- **M2-A gate**：S38-S42 + S1-S37 + fmt/check/strict clippy/compliance + 真实 browser golden + `m2-a-acceptance-report.md`。
- **M2-B gate**：S43-S47 + M2-A/M1/M0 全回归 + `m2-b-acceptance-report.md`。
- **M2-C gate**：S48-S52 + M2-A/B/M1/M0 全回归 + `m2-c-acceptance-report.md`。
- **M2 final**：S1-S52、真实世界 typed eval/portable trace、secret scan、合规门和 `m2-acceptance-report.md` 全部完成。

任何场景只验证 mock driver、最终回答或页面文案而没有 Harness/event/store 证据，不得计为集成通过。确定性 fake 可用于单元/反向测试，但至少一个 real-driver golden 必须单独保留证据。
