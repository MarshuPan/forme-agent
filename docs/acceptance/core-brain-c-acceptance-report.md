# V1 核心大脑闭合 C 波（项目与编排）分支验收报告

- 日期：2026-10-17。
- 分支：`shu/v1-core-brain-a`；基线 `main@94fe6f1`，本波未合入 `main`。
- 权威规格：`requirements/16` C13–C18、canonical §28、`architecture/09`、`prd/23` §5 C1–C6。
- 结果：**C1–C6 分支实现与 C13–C18 整波自检 PASS，干净树 S1–S99 与发布门 PASS；待 owner 验收。** D 波和 V1 Final 不在本报告的通过范围。

## 1. 模块与提交

| 模块 | 提交 | 已接入的行为 |
|---|---|---|
| C1 WorkspaceCharter | `326f281`、`db29cbb` | owner/CAS/命令幂等、事务内事件与投影、声明引用校验、正式 context、路线与最终 action 出口重查 |
| C2 八套模式 | `3a983b4`、`db29cbb` | purpose/roles/guide/anchors/fitness 索引；前四套零 child，后四套正式 DAG 执行 |
| C3 六维匹配 | `3a983b4`、`db29cbb` | typed 维度分数、hard filter、阈值、低置信 custom review route；不按任务字符串映射 |
| C4 Route runtime 与监管 | `db29cbb` | durable node claim、模型调用额度预留、依赖失败、取消/重试/unknown、真实目录隔离、审批恢复、checkpoint 与 tick 重规划 |
| C5 经验回流 | `db29cbb` | 成功/失败/取消/未知 outcome 的 episode 与 typed lineage；strategy/fitness evidence 仅到 candidate，不自动激活 |
| C6 证据与永久门 | `0c1a3f4`、`51db1aa` | 六份 typed artifacts、独立离线 verifier、35 个 exact 按名测试、临时与仓库工件同时验证 |

承重协议与执行细节先于实现登记于 `a703423`、`fb28755`、`3454fc8`、`9d7c040`。跨 crate 的协议载荷、投影和正式运行时作为一个可编译整体提交；代码、生成工件和门禁各自提交。

## 2. C13–C18 的逐项证据

`tools/verify-core-brain-closure.ps1 -Wave C` 精确运行 35 个命名测试，每次要求恰好 1 个通过；测试缺失或零测试不能通过。分组为 charter 2、正式 runtime 23、library 2、protocol 2、Gateway 1、portable evidence 4、最终输出验证 unit 1。

| 场景 | 入口 | 核心正反断言 |
|---|---|---|
| C13 Charter | harness `v1_core_brain_c_charter` 与 `v1_core_brain_c_orchestration`；Gateway `gatewayd_e2e` | owner/CAS/幂等/重启；跨 workspace、失效引用和未知规则零写入；context 可见；charter 收紧使已有 child approval 无效；direct 项目也不能借用主脑未分配的 capability |
| C14 八套模式 | coordination `coordination_contract`；harness 八模式正式运行场景 | exact seed set、索引完整、前四套零 child、后四套按声明依赖；Tool Use/Memory 不伪装为同级编排条目 |
| C15 匹配/custom | coordination 分数场景；harness clarify/custom 场景 | 六维 breakdown、hard filter、阈值、anchors 与 active fitness；未澄清零 claim/model/driver；全不匹配时低置信 custom + 独立 review，完成后仍只生成候选 |
| C16 运行治理 | protocol `v1_orchestration_contract`；harness route/tool/recovery 场景及最终输出 unit；Gateway 进程场景 | Pending/Running/Done/Failed/Cancelled/OutcomeUnknown replay；并发单 claim、预算原子预留/回收、依赖失败、相同 intent 幂等、受限重试、取消不复活；真实子目录及别名逃逸拒绝；审批前零文件修改，批准后一次；child 审批跨重启恢复且不重复模型/副作用；crash 后 unknown 保留预留且不重试；事实/设计/质量冲突保留上抛 |
| C17 长期监管 | harness 三个 C17 场景 | direct 起步，checkpoint 后以 evidence/failure/deadline/cadence 再 frame/plan；运行中/Waiting/Unknown 不热换；无变化/未到期零新 route；晚到证据只进入下一轮基线；监管只规划，不执行项目动作 |
| C18 经验回流 | harness C18 场景；portable evidence | 成功与失败都有 episode、goal/pattern/node/result/failure/verification lineage；strategy candidate 置信度 0.4，active fitness 保持 0.5；SQLite replay 与共享 memory cache 一致；旧 route 补尾不覆盖更新 checkpoint |

Gateway 场景运行真实 `forme-gatewayd` 子进程，验证 Bearer/Origin/CSRF、未知字段拒绝、项目登记/规划/执行/取消、零 child direct 和重启幂等。它使用同一 ProductionRuntime/authority，不建立入口私有的调度器或认知实例。

## 3. 可移植工件

目录：`core-brain-c-artifacts/`；生成源码：`db29cbb83f044eff8baf05f08e179295de229574`。证据等级固定为 `controlled-http-real-runtime-sqlite-files`，即受控 HTTP 模型、正式 runtime、真实 SQLite/文件/进程。六份逐一通过 `forme-core-brain-c-artifact-verify`：

| 文件 | 事件见证数 | 内容 digest |
|---|---|---|
| `patterns.json` | 568 | `sha256:a5921c93e18143ddab1d447b483293a051dbfac83064a5dbda8d37d43e9fa2ad` |
| `conflict.json` | 99 | `sha256:e5efc1e2583a67f8a1a12ad31cdf0621dad4091d32d42c9e306c9039a68e5985` |
| `recovery.json` | 43 | `sha256:7ec86a2d1cbaa3858885c23ae4859e6fb59bdfac5f33445513f35079737d5175` |
| `isolation.json` | 127 | `sha256:41891294692bf19e47d749f0e0affb630e953220d9263038386c4332c7f6637f` |
| `supervision.json` | 79 | `sha256:eb6d52ece435f7a74bce7c76b4935ab02aacfd50bdc4172dff230c901454b58a` |
| `learning.json` | 146 | `sha256:2c2f90801a9046819cdab3bec292ad4c39b0d7f035c2f0f8c7c65229ef9ee14e` |

每份有版本、源码提交、composition/99-kind taxonomy digest、实际 profile 映射、各 stream 完整序号/hash 见证、安全载荷、在线 ledger digest 与相对路径文件 hash。离线 verifier 不访问 owner store、模型或 driver；重放节点/预算/终态，核对调用数、候选和文件记录。负场景覆盖缺失序号/载荷、伪预算、改 digest、未知字段、秘密/本机路径、profile/schema 与文件 hash 篡改。

敏感 tool/approval 载荷只保留 hash 见证；这不等于可从工件恢复全部原始事件。文件 hash 是运行时对真实文件的捕获，离线验证不重新读取本机文件。受控模型正确返回 fixture 结果只证明协议/运行链，不证明线上认知质量或编排优于直接模型。

## 4. 工程与回归门

| 命令 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test --workspace --all-targets` | PASS：528 通过、0 失败、4 默认 ignored；真实 golden 由 M5 门显式运行 |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `bash tools/compliance-doctor.sh` | PASS：upstream/license/copy |
| `tools/verify-core-brain-closure.ps1 -Wave A` | PASS：43 个按名场景 |
| `tools/verify-core-brain-closure.ps1 -Wave B` | PASS：48 个按名场景及仓库相关性工件独立校验 |
| `tools/verify-core-brain-closure.ps1 -Wave C` | PASS：35 个按名场景及新生成/仓库六份工件独立校验 |
| `tools/verify-m5.ps1` | PASS：干净检出 `854ee23`；S1–S99、显式真实 golden、528 项 Rust 测试、31 项 Python 检查、strict Clippy、typed artifacts、当前树审计与 receipt 比对 |
| V1 Final gate | D 波未实施，必须继续 fail closed，不作 Final 通过声明 |
| `git diff --check` | PASS |

专项及工程日志位于本地主目录 `target/v1-c-full-tests.log`、`v1-c-clippy.log`、`v1-c-compliance-final.log`、`v1-c-wave-A.log`、`v1-c-wave-B.log`、`v1-c-wave-C-final.log`。这些本地日志不是仓库可移植工件的替代物。

正式 M5 脚本按验收仓库本地 `target/debug` 解析进程路径，运行时应在该验收子进程清除 `CARGO_TARGET_DIR`。首次共享构建目录的尝试已运行到 M5 进程构建，但因本地路径缺少 executable 停止；其日志 `target/v1-c-m5-full.log` 不作为完整 PASS 证据。后续完整重跑使用验收检出自己的构建目录，输出为 `target/v1-c-m5-full-local-target.log`。

完整重跑已在 `854ee23` PASS，含 M2/M3 真实浏览器、M4 三进程 federation、M5 authority/registry/executor golden 与仓库/新生成工件独立校验。干净检出的 A/B/C 专项日志为 `target/v1-c-clean-wave-A.log`、`v1-c-clean-wave-B.log`、`v1-c-clean-wave-C.log`；Final 预检单独保存在 `v1-c-final-preflight.log`，D inventory 缺失应明确拒绝。

本报告与四个索引只补记已验证行为，不改变源码、配置或测试门。最终文档树的 M5 receipt 在本报告之后独立提交；发布证据以该 receipt、干净树审计、独立验证和当前树比对同时通过为准，不以旧 receipt 替代。receipt 目录排除自身 digest，自引用提交号不写入本报告。

## 5. 契约与使用边界

- 保持 18 crates、内部依赖边和 99 EventKinds；前 97 为 M5 exact prefix，仅既定两个 R 事件位于末尾。没有增加内核或里程碑编号。
- 编排事实使用既有 `DecisionTraceRecorded` 的 optional typed 载荷；历史字段缺失时保持旧语义。store 单写者事务校验 command、expected version、charter 和预算，再 append/fold；稳定状态没有旁路 setter。
- owner 项目控制为 `GET/POST /v1/brain/projects`；POST 形如 `{schema_version:1, request:{operation:register|revise|plan|execute|cancel,...}}`。审批中的 child 沿既有 Gateway resume/cancel 路径可达。
- `budget_units` 是项目模型调用次数额度，不冒充费用/token 或主脑总思考上限。每次 attempt 先预留，真实 `ModelCallStarted` 计消费；Waiting/Unknown 保留预留，只退回可证明未使用的部分。
- child 只获得分配的 scope/toolset/model、依赖 summary 和验证结果，文件目录按 route/node/attempt 隔离。shell/MCP/网络/连接器不隐式继承；模型参数不授予权限。
- 本波真实 child 工具证据集中于 File；其他 backend 的实际行为仍需结合历史专项与最终任务场景判断，不能从受控模型输出外推为所有工具均已完成线上实测。
- runtime 发送本次实际进入 context 预算的工具目录，安全函数别名只在该目录中反解。正式 binding 决定 capability/backend/scope/risk/permission，最终 driver 前重查当前 policy/charter/route；重启不刷新默认授权起点。
- `RouteClosed` 不等于 checkpoint；只有 checkpoint 允许下一 segment。OutcomeUnknown 不 checkpoint、不自动重试，也没有在本波新增“裁决后自动继续 DAG”的接口。
- 本波未实施 D 波 C19–C24 的多模型深化、dashboard、备份/加密/真实删除或最终六任务长期价值 golden；未声称 V1 可打包日常使用。
- 工作目录原有 `Cargo.toml`、`LICENSE`、`NOTICE`、`README.md`、`tools/release_audit.py`、`tools/tests/test_release_audit.py` 六个许可证相关修改保持原样，不纳入本波提交。正式发布门使用只含已提交实现的隔离干净检出，不豁免 dirty-tree 检查。
