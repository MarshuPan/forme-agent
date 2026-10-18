# V1 核心大脑 D 波实现与验收报告

- 日期：2026-10-19。
- 分支：`shu/v1-core-brain-a`；未合入 `main`。
- 契约：`requirements/15-16`、canonical §28、`architecture/09`、`prd/23` D 波。
- 结论：**C19–C23 实现与 D 波 70 项运行场景自检 PASS；C24 真实配置模型不可用，保持 UNVERIFIABLE；D 整波与 V1 Final 尚未通过。**

## 1. 提交与范围

| 提交 | 内容 |
|---|---|
| `fbe1ac7`、`01fdd83` | 每 attempt 绑定实际 provider；同显式 group 的有界 fallback、传输错误分级及诊断脱敏。 |
| `872e8dd`、`7434576` | 事件派生认知看板、scope/time-window/历史状态与反馈归属；C24 typed artifact/独立校验器。 |
| `fa94d83` | SQLite 一致性备份、manifest/digest/replay 校验及恢复到新目标。 |
| `e616713`、`786d323` | AEAD envelope、所有正式内容列保护、旧明文离线迁移、真实密钥销毁与可恢复 lifecycle。 |
| `10b3115` | 同一 ProductionRuntime 的 owner data control/Gateway/CLI、清理 fence/cache 刷新、真实任务 runner。 |
| `8589918` | D 的精确运行清单、live-only 正式门与独立 closure release audit。 |
| `fd791e2` | 数据控制 companion、D 证据规则和操作手册；文档单独提交。 |

保留 18 crates、既定内部依赖边和 99 EventKinds；前 97 项保持 M5 exact prefix。本波没有新增内核或里程碑编号。原工作区已有的许可证相关修改没有混入上述提交。

## 2. C19–C23 的实现与反向证明

| 场景 | 已验证行为 | 关键拒绝条件 |
|---|---|---|
| C19 多模型 | 调用前选定具体 provider；实际 profile/usage/失败逐 attempt 入事件。最多 primary + 2 个显式同 group fallback；M3 adaptation 绑定所选 profile 自己的实测证据。 | timeout/unknown、高风险历史、能力不兼容、无同 group 或链耗尽不切换；provider 文本不能伪造 retryability；不能借另一个模型的 measurement 激活 scaffold。 |
| C20 看板 | global/workspace/time-window；窗内行为与截至窗尾 backlog/confidence/监管状态分开；反馈追溯原 proposal；各 stream heads 与 digest 可重放。 | 不把配置的 fallback 当发生过的 fallback；owner 级事实不伪分给项目；学习回答被接受不等于知识有用，reject 不等于 regret。 |
| C21 备份恢复 | SQLite backup API、一致性 manifest/hash/integrity/事件连续性/replay；owner 批准不可变 plan 后恢复到新文件，one-shot 持久消费。 | 篡改 DB/manifest、变化的 source、现存 target、路径跳转、异 owner/digest/nonce/expiry 拒绝；不自动切 authority，不盲重试未知结果。 |
| C22 内容保护 | ChaCha20Poly1305 的真实 AAD 认证绑定 owner/scope/object/schema；正式 event、session、charter、route、strategy/evaluation、辅助内容列与外置附件/截图被保护；旧明文经离线 operator 迁移。 | 错 key/缺 resolver/篡改/行绑定替换/未处理 schema 拒绝；启用保护拒绝历史明文混存；迁移不覆盖目标、不接管 authority，源保持不变并明确仍未保护。 |
| C23 数据生命周期 | scope/version/plan-bound cleanup；移除投影/检索入口，沿真实 lineage 使派生后缀失效，销毁历史 key，最后落成功事件/tombstone；在线缓存刷新，重启不能复活。 | 清理中断保持同 digest pending job、零成功事件；正式 run/tick/项目/缓存读被 fence；跨 scope 引用与活动/挂起执行拒绝；旧备份不能绕过已销毁 key。 |

Gateway mutation 均经 owner Bearer、Origin、CSRF 与 closed DTO；CLI 经真实 daemon 调用同一 runtime，没有新建私有 SQLite writer。离线迁移是单独 operator，要求停 daemon、源 authority owner 一致、批准 digest/expiry 和新目标。

## 3. 已执行检查

| 检查 | 本轮结果 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test --workspace --all-targets` | PASS：593 passed、0 failed、5 默认 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `bash tools/compliance-doctor.sh` | PASS |
| workspace graph / taxonomy contract | PASS：18 crates / 99 kinds / 97 strict prefix |
| `verify-core-brain-closure.ps1 -Wave D -RuntimeOnly` | PASS：70 项逐名运行，包括默认忽略的真实 CLI/data case |
| `tools.tests.test_core_brain_release_audit` | PASS：21 项 Python tests |
| A/B/C 永久 closure 回归 | PASS：A 43、B 48、C 35 项逐名运行；B/C 仓库工件与新生成 C 工件独立校验通过 |
| 干净检出 `verify-m5.ps1` / S1–S99 | 正在复跑；发布 receipt 需绑定最终树 |
| C24 runner / `--require-pass` | exit 2：Unavailable / Unverifiable |

Windows 首次全量并发构建触发系统 paging/metadata mmap 失败，不作为源码失败或通过。后续以 `CARGO_BUILD_JOBS=2` 完整运行成功；不得把前一次不完整日志作为 PASS。另一轮 Gateway fixture 临时目录碰撞已用 PID/time/AtomicU64 修复后重跑。

原工作区日志在 OS temp：`forme-d-workspace-2d9bdfa96145455f96a9d85852a32207.log` 与 `forme-d-runtime-a89f38d01859497984ad27079c7a7731.log`；这些本机日志不是便携工件或跨机器通过证明。正式发布检查使用独立干净检出及该 checkout 自己的 `target/debug`，清除共享 `CARGO_TARGET_DIR`。

A/B/C 本轮专项日志：`forme-d-regression-abc-7a2377ebf6ba475285d141f0ed73b8bd.log`。C24 产品工件 digest 为 `sha256:9367f6f43292c61967d0fe9cd24be92c6ef300312fba095be701582929c7d340`；`core_brain_release_audit.py --verify-product` 明确返回 `CORE_BRAIN_C24_UNVERIFIABLE` 与 exit 2，未将零试验误判为通过。

## 4. C24 与剩余门

已提供六类固定工作流的 production runner：冷启动、文件工具、跨 SQLite 重启的长期 workspace、多 child、AskToLearn、失败后重协调；每类至少两次，另有 direct-model 答案质量基线。typed verifier 核对固定 selector、完整 stream 位置/内容 hash、实际 profile/call/usage、approval/plan/scope/expiry、route ledger、文件与重启证据。

当前 `core-brain-d-artifacts/` 是正式 runner 实际生成的 **Unavailable** 工件，源码 `8589918`，六类均为零次试验。缺凭证不能升级为 live，受控 HTTP 也不能通过 C24；`-RuntimeOnly` 仅证明运行与治理自检。

剩余动作：配置支持工具调用的真实模型，在新目录运行 runner，核对真实结果并按失败事实修复，独立校验 PASS 后更新便携工件；随后 D 正式门、current-tree release receipt 和 Final 门才能通过。最终与 `main` 合入分别记录。

## 5. 实际边界

- 本地应用层加密/crypto-shred 不证明物理扇区、内存、用户自行另存原文/密钥或未验证远端已删除；远端保持 Unknown。
- 备份不含 key。恢复需正确 resolver；新 DB 不会自动获得旧 `.content-keys`，authority 切换必须显式执行。
- 迁移成功的新库不等于旧源、失败目标或旧备份已清除；它们仍需 owner 管理。
- 六类短周期基准不证明通用复杂任务能力、编排优于直接模型或数月主动价值。direct baseline 不完成相同工作流；activation/memory/routing 等机制的独立对照不能由共同算术答案替代。
- C24/D/Final 未通过前，不声明 V1 可打包日常使用。
