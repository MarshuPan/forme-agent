# V1 核心大脑 D 波实现与验收报告

- 日期：2026-10-19。
- 分支：`shu/v1-core-brain-a`；未合入 `main`。
- 契约：`requirements/15-16`、canonical §28、`architecture/09`、`prd/23` D 波。
- 当前状态：**七项默认大脑补闭已实现，四波及真实 C24 通过，Final 与发布回执正在重验。** 当前源码为 `b9ea670`；本轮记录见 §7。§3–5 仅记录 `025ad3d` 及更早源码的历史验收，不作为本轮通过证明。owner 人工验收与 `main` 合入仍分别记录。

## 1. 提交与范围

| 提交 | 内容 |
|---|---|
| `fbe1ac7`、`01fdd83` | 每 attempt 绑定实际 provider；同显式 group 的有界 fallback、传输错误分级及诊断脱敏。 |
| `872e8dd`、`7434576` | 事件派生认知看板、scope/time-window/历史状态与反馈归属；C24 typed artifact/独立校验器。 |
| `fa94d83` | SQLite 一致性备份、manifest/digest/replay 校验及恢复到新目标。 |
| `e616713`、`786d323` | AEAD envelope、所有正式内容列保护、旧明文离线迁移、真实密钥销毁与可恢复 lifecycle。 |
| `10b3115` | 同一 ProductionRuntime 的 owner data control/Gateway/CLI、清理 fence/cache 刷新、真实任务 runner。 |
| `8589918` | D 的精确运行清单、live-only 正式门与独立 closure release audit。 |
| `3289ed2` | 对齐 `chacha20poly1305` 许可证顺序与 Cargo metadata；干净检出 M5 release audit 复核通过。 |
| `fd791e2` | 数据控制 companion、D 证据规则和操作手册；文档单独提交。 |
| `6ee8ea4`、`1fdf884`、`1d1f671` | 四波封闭清单、逐名运行 receipt、A 正式入口与 B 三相 tick 见证及汇总校验。 |
| `547d08b`、`5ffcf74` | 先约定原生工具往返关联，再修复 assistant tool_calls/tool_call_id、审批恢复续接；严格 HTTP fixture 验证实际消息。 |
| `1a79b57`、`8e31e21` | 扫描 JSON 解码后的私有路径/URL；Git 保留证据原始字节，避免换行破坏摘要。 |
| `62cba47`、`025ad3d` | 区分正式 owner model turn 与 Schedule 通知的受治理 action；真实导出回归与漏门/伪造路径负测试。 |

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

## 3. 首次 D 波检查（历史记录）

以下是配置真实模型前的历史检查；当时 C24 为不可验证，没有回写成通过。最新重验见第 4 节。

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
| 干净检出 `verify-m5.ps1` / S1–S99 | M5 永久回归与发布审计复核 PASS；M5 receipt：`sha256:54580676428efeb52a146dbc349a6dd4ac84eddf97680adf3634e5fc14ce67ba`。该 receipt 属于 M5 历史门，不等于 V1 Final。 |
| C24 runner / `--require-pass` | exit 2：Unavailable / Unverifiable |

Windows 首次全量并发构建触发系统 paging/metadata mmap 失败，不作为源码失败或通过。后续以 `CARGO_BUILD_JOBS=2` 完整运行成功；不得把前一次不完整日志作为 PASS。M5 首次发布审计发现 AEAD 许可证顺序漂移，已在 `3289ed2` 修正并用最新干净检出复核通过。另一轮 Gateway fixture 临时目录碰撞已用 PID/time/AtomicU64 修复后重跑。

原工作区日志在 OS temp：`forme-d-workspace-2d9bdfa96145455f96a9d85852a32207.log` 与 `forme-d-runtime-a89f38d01859497984ad27079c7a7731.log`；这些本机日志不是便携工件或跨机器通过证明。正式发布检查使用独立干净检出及该 checkout 自己的 `target/debug`，清除共享 `CARGO_TARGET_DIR`。

A/B/C 本轮专项日志：`forme-d-regression-abc-7a2377ebf6ba475285d141f0ed73b8bd.log`。C24 产品工件 digest 为 `sha256:9367f6f43292c61967d0fe9cd24be92c6ef300312fba095be701582929c7d340`；`core_brain_release_audit.py --verify-product` 明确返回 `CORE_BRAIN_C24_UNVERIFIABLE` 与 exit 2，未将零试验误判为通过。

## 4. 前轮真实 C24、修复与重验（历史：025ad3d）

已提供六类固定工作流的 production runner：冷启动、文件工具、跨 SQLite 重启的长期 workspace、多 child、AskToLearn、失败后重协调；每类至少两次，另有 direct-model 答案质量基线。typed verifier 核对固定 selector、完整 stream 位置/内容 hash、实际 profile/call/usage、approval/plan/scope/expiry、route ledger、文件与重启证据。

首次真实运行绑定源码 `1d1f671`：五类各 2/2 通过，但工具类两次真实写文件后均无法完成模型续接，verifier 返回 `Unverifiable` / exit 2。原因是发送了孤立 `role=tool`，缺少原始 assistant 调用与 call ID。该记录保存在 `../archive/acceptance/core-brain-c24-1d1f671/`；未算作成功，也没有降低 selector。

`5ffcf74` 在单工具契约内保存原生调用 ID/参数，工具结果与 assistant 调用关联，审批等待及恢复后仍保持关联；孤立/错配/重复/未完成的往返在 provider 边界拒绝。当前不支持的多个工具 proposal 在产生副作用前拒绝，非原生 handoff 不伪造 tool result。

工具修复后的真实重验通过。随后封存发现 A 入口校验器错误地要求到期通知也调用模型；`025ad3d` 改为分别检查对话与受治理工具路径，没有改 runtime 或放松主动门/策略/完成证据。因为工具也属于源码输入，又从干净验收检出重新采集全部四波与真实 C24，当前受理源码为 `025ad3dca3c84e7e10a7eb937a44148c599fa560`，实际 profile 为 `compatible:gpt-6.1-sol`。六类各两次；正式 runner 与独立 `forme-core-brain-d-artifact-verify --require-pass` 均返回 0。

| 类别 | 答案质量 | direct-model 质量基线 | 工作流 | 不可验证 |
|---|---|---|---|---|
| ColdDialogue | 2/2 | 2/2 | 2/2 | 0 |
| ComplexTools | 2/2 | 2/2 | 2/2 | 0 |
| LongWorkspace | 2/2 | 2/2 | 2/2 | 0 |
| MultiAgent | 2/2 | 2/2 | 2/2 | 0 |
| AskToLearn | 2/2 | 2/2 | 2/2 | 0 |
| FailureRecoordination | 2/2 | 2/2 | 2/2 | 0 |

当前 `core-brain-d-artifacts/` 的产品工件为 `LiveConfigured / Pass`，内容 digest：`sha256:027af5b21114b0e4da85950e49fa6cbb8feaefbb2c094847ead9a1ee7b23e7b7`。价格未配置，费用保持 unknown；不把它写成免费。端点与凭证没有进入便携工件；本机模型配置继续只留在忽略目录。

本轮修复后完整工作区测试为 **597 passed、0 failed、5 默认 ignored**；fmt、check、strict Clippy、compliance 均 PASS。A/B/C/D runtime 按冻结名称重新采集为 **43/48/35/70** 项，来源输入一致；D runtime 不单独代替 live C24。证据工具 Python 12 项（11 通过、1 项因 Windows 无符号链接权限跳过）、core release audit Python 22 项通过。

四波与总清单已经封存，A/B/C 为 `ControlledRuntime`、D 产品工件为 `LiveConfigured`。总清单 digest：`sha256:fd30e7ba6fd4b3873824f8a8ebb6764db055c1d049310f9c6b3dd844ae5be816`。每波均有非空事件范围及快照引用，所有文件通过隐私扫描与独立内容/来源校验。

## 5. 前轮最终发布总门（历史：025ad3d）

2026-10-19 13:05（Asia/Shanghai），干净验收检出 `03d2159` 执行下列完整命令，实际返回 **0**，末行为 `core-brain-closure (Final): PASS`：

```powershell
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_TARGET_DIR = Join-Path $pwd 'target'
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-core-brain-closure.ps1 -Wave Final
```

总门包括 fmt/check/完整 workspace tests/strict Clippy/compliance、M0–M5 的 S1–S99、真实 Chrome/API/M3 evolution/M4 federation/M5 ecosystem golden、A/B/C/D 的 43/48/35/70 项逐名场景、B/C/D typed verifier、四波封闭清单以及当前干净树发布回执生成/独立校验/仓库比对。18 crates、99 EventKinds 与前 97 exact prefix 不变。Python 总门 65 项：64 通过，1 项因 Windows 符号链接权限跳过；5 个默认忽略的 Rust case 由相应场景门显式执行，不把默认 ignored 当作通过。

core release receipt 为 `sha256:ad82603edab268efef0bf7b712738ae11201b60a1c57c01df04cf0a5ae68aaae`，当前清单见 `core-brain-closure-release-audit-artifacts/`。M5 current-tree receipt 见 `m5-release-audit-artifacts/`；该回执会随本报告变动刷新，不能将历史摘要固定写回报告造成循环依赖。两个当前报告与 M5 receipt 被 core 根摘要显式排除，受测 `crates/tools/Cargo.toml/Cargo.lock` 输入仍严格绑定 `025ad3d`。

本次总门日志在 OS temp，名称为 `forme-v1-final-gate-025ad3d-complete.log`；原始构建路径只留本机，不作为便携证据。报告/回执收口只变文档与工件，继续核验当前源码一致性、四波清单和发布回执。首次失败归档与旧回执由 Git 历史保留，不以最新结果改写旧事实。

验证平台为本机 `x86_64-pc-windows-gnu`。审计对非本目标的已知依赖告警保留 observation；本报告不声称其他操作系统/目标已经运行验收。原工作区六项既有修改未纳入，模型配置保留在忽略的本机文件中；`main` 仍为 `94fe6f1`，未合入本分支。

## 6. 实际边界

- 本地应用层加密/crypto-shred 不证明物理扇区、内存、用户自行另存原文/密钥或未验证远端已删除；远端保持 Unknown。
- 备份不含 key。恢复需正确 resolver；新 DB 不会自动获得旧 `.content-keys`，authority 切换必须显式执行。
- 迁移成功的新库不等于旧源、失败目标或旧备份已清除；它们仍需 owner 管理。
- 六类短周期基准不证明通用复杂任务能力、编排优于直接模型或数月主动价值。direct baseline 不完成相同工作流；activation/memory/routing 等机制的独立对照不能由共同算术答案替代。
- 有界验收通过不等于长期日常使用质量已经证明；Final 发布门和 `main` 合入分别记录。

## 7. 七项默认大脑补闭后的当前源码重验

实现提交 `b9ea670accdae0f59122aa62333cb172e18416e4` 补齐多 workspace 共享大脑、普通运行到三模型候选、稳定认知正文进入 context、统一 governor 提升、实时能力输入、默认 M3 正文/seed/回滚及受治理路线适配。逐项行为、七个新增永久测试与反向断言见 `core-brain-closure-acceptance-report.md` §1；冻结契约与运行手册分别见 `prd/23` §1.0 和 `runbooks/v1-core-brain-data-and-evidence.md` §8。

从干净验收检出重新采集 A/B/C/D 为 **47/50/36/71** 项逐名运行，全数通过。C 的六份新工件分别通过独立 verifier。源码、场景清单和文件摘要已重新封存，汇总 digest 为 `sha256:b957358c7b639fdca4863de548ccd187a4fd7a55d9fdaedb3290f2c016f276db`，四波封闭清单 `--verify --require-pass` 返回 0。

同一源码的真实 C24 六类各两次，质量、直接模型质量基线与工作流均各 2/2，不可验证为 0；正式 runner、独立 typed verifier 与源码一致性 verifier 全部返回 0。新的产品 digest 为 `sha256:9e7c0a2a03cfee4f2fccb40754baa42383f1bf48acfdbc5a1777f55246ef647a`。凭证与端点未导出，价格未知仍为 unknown。该证据不证明长期认知收益；受控集成测试也不自动成为 live 质量证据。

本轮 Final 正在重跑，core 与 M5 current-tree receipt 依次重封。旧 §5 的回执仅保留历史事实；完整总门返回码和本轮回执完成后更新本节。两个当前报告被 core 根摘要明确排除，最终报告收口仍须刷新受影响的 M5 receipt 并复核当前根摘要。原工作区六项既有修改不纳入，`main` 未动。
