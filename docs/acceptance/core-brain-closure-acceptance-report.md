# V1 核心大脑运行闭合：整体验收状态

日期：2026-10-20。当前分支：`main`；已从 `94fe6f1` 快进合入 `shu/v1-core-brain-a@a1fe0df`，保留完整分波提交历史。

**七项默认大脑补闭与 A–D 已通过技术 Final，并合入本地 main。** 历史 Apache-2.0 输入及其验收结果保留在 §2–3；MIT 发布输入的当前重验见 §5。owner 日常使用验收、发布包与远端发布分别记录。

## 1. 本轮补齐的七项运行链

| 缺口 | 已实现行为 | 永久反向或恢复证明 |
|---|---|---|
| 一份 owner 大脑下的多 workspace | 使用同一 BrainServices 与持久 cognition；owner 登记的 workspace 分别拥有项目、charter、会话和资源约束。 | 两个项目实际运行并跨重启重放；登记工作区不会获得新工具权限，也不能跨目录执行。 |
| 普通运行进入三模型候选 | 可信输入、任务结果、验证/失败及反馈进入 UserModel、AgentSelfModel、PartnershipModel 候选链。 | 同一终态再次观察不重复写事实；重启保留候选；最终回答不能冒充能力证据。 |
| 稳定认知进入真实 context | 读取有效 scope 的正文、种类、置信度、稳定性和 evidence，正式 HTTP 模型请求实际包含正文。 | 候选不会冒充稳定知识；撤销后正文与相应能力输入消失，其他 scope 的稳定认知不可借用。 |
| 统一提升与纠正 | 正式候选审核进入同一个 EvolutionGovernor，提升携带可重放 StableCognitionRecord。 | owner 确认不能代替结果证据；跨 stream 的真实证据可解析，异 scope 证据不能抬高认知。 |
| 出口读取实时能力证据 | 每次执行及审批恢复重新读取 capability/scope 对应的已验证结果、失败、自我认知与 trust。 | 同一结果的两种视图只计一次；新失败或撤销会降低上限；重启保持一致。 |
| M3 接入默认装配 | 六个 domain runtime 使用版本/作用域/摘要绑定的不可变正文；正式 owner seed/spec 控制进入同一 ProductionRuntime。 | 首次 seed 为内置行为等价初始化；不同正文不能覆盖旧版本；已激活 Loop 参数影响实际 run，重启与回滚可用。 |
| 经验影响后续编排 | Coordination 只采用已激活候选明确引用、早于候选快照的同类路线结果，成功与失败共同影响适配分数。 | 未激活或晚到经验不改变 active policy；激活后选择变化可重放。 |

新增永久测试位于 `crates/harness/tests/v1_brain_integration_closure.rs`（5 项）与 `v1_brain_competence_closure.rs`（2 项）；Gateway 真实 daemon 测试同时覆盖 owner 认证的 seed/spec 接口。七项检查归入现有 C1–C24，不新增里程碑或场景编号。

## 2. 补闭轮四波和真实模型证据（历史输入 b9ea670）

| 波 | 本轮逐名执行 | 证据与边界 |
|---|---|---|
| A：C1–C6 | 47 项 PASS | 正式装配、共同入口、owner/workspace、context 与权限；entrypoints 见证为 ControlledRuntime。 |
| B：C7–C12 | 50 项 PASS | 三模型、实时能力、三相 tick、学习、扩散/直接模型对照和维护；保留扩散不如直接模型的受控负结果。 |
| C：C13–C18 | 36 项 PASS | 项目、八个编排种子、DAG 治理、监管与受治理经验；六份新生成的 SQLite/文件/事件工件由独立 verifier 通过。 |
| D：C19–C24 runtime | 71 项 PASS | 真实 profile、数据控制、备份、加密、删除、默认 M3 和工具审计；RuntimeOnly 本身不关闭 C24。 |
| C24 live | 六类各 2 次 PASS | 冷启动、工具、长期 workspace、多 child、AskToLearn、失败重协调，质量/直接模型质量基线/工作流均各 2/2；不可验证为 0。 |

C24 来自干净源码 `b9ea670`，正式 runner 返回 `Pass (LiveConfigured)`；独立 `forme-core-brain-d-artifact-verify --require-pass` 与 `core_brain_release_audit.py --verify-product` 均返回 0。产品内容 digest：`sha256:9e7c0a2a03cfee4f2fccb40754baa42383f1bf48acfdbc5a1777f55246ef647a`。

四波和汇总清单已更新：`core-brain-closure-artifacts/manifest.json`，digest：`sha256:b957358c7b639fdca4863de548ccd187a4fd7a55d9fdaedb3290f2c016f276db`；`tools/core_brain_evidence.py --verify --require-pass` 返回 0。所有运行 receipt 均绑定同一源码输入和当前 204 项冻结清单；工件保留事件位置、快照、内容摘要和隐私校验。A/B/C 不因 D 使用真实 provider 而升级为线上证据。

## 3. 补闭轮 Final 与发布回执（历史输入 b9ea670）

本轮在干净检出 `5e4f69788bc54ec859b034c4092f0d5f66ee9168` 执行下列完整命令，实际返回 **0**，末行为 `core-brain-closure (Final): PASS`：

```powershell
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_TARGET_DIR = Join-Path $pwd 'target'
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-core-brain-closure.ps1 -Wave Final
```

完整工作区为 **604 passed、0 failed、5 默认 ignored**；这 5 项真实进程用例均由各自场景门显式运行。fmt/check/strict Clippy/compliance、S1–S99、真实 browser/API/evolution/federation/ecosystem golden、四波 204 项逐名执行、独立 typed verifier 和当前干净树 core/M5 receipt 重算比对全部通过。Python 工具门 65 项：64 通过，1 项因本机 Windows 无符号链接权限跳过。

本轮 core receipt 为 `sha256:2caabec7c64f7ea047115463f95e6b164f0a647799c200734a81e6af16881b11`，目录 `core-brain-closure-release-audit-artifacts/`。M5 current-tree receipt 见 `m5-release-audit-artifacts/`；报告收口后单独刷新该回执，不把其摘要写入受它约束的报告。两个当前报告与 M5 receipt 被 core 根摘要明确排除，报告收口后继续验证该根摘要、源码和四波清单一致。

证据更新期间的第一次总门未通过发布审计，返回 1，未计为 PASS；先提交新证据并重封两类回执，再从干净检出完成本轮总门。完整成功日志 `forme-v1-final-gate-b9ea670-614f216bccc9409198f9a976961fc13f.log` 保留在本机 OS temp；便携证明是已提交的 typed 工件和封闭清单。

此前 2026-10-19 13:05（Asia/Shanghai）`03d2159` 的完整 Final 返回 0，源码输入是 `025ad3d`，四波为 43/48/35/70 项。它与首次真实工具调用失败、修复后的历史通过结果均保留，不改写成当前源码证据；详细历史见 `core-brain-d-acceptance-report.md` §3–5。

## 4. 验证范围与实现限制

本轮保持 18 crates、99 EventKinds、前 97 exact prefix 与既定依赖方向。实现提交为 `dd07152`、`b9ea670`；契约、操作文档、数据和验收报告分别提交。原工作区六项许可证相关既有修改逐文件 hash 保持不变；本机模型配置与密钥未进入 Git 或便携工件。

七项新增测试使用受控 HTTP、正式运行时、真实文件和 SQLite 重启，证明接线与治理。真实 C24 的六类短周期工作流另行验证；两者均不证明数月认知收益或编排普遍优于直接模型。当前项目规划仍接收类型化任务签名，路线经验采用受治理的成功/失败适配分数，不将其描述成通用自然语言到 DAG 推断、成本最优化或完整在线优化器。首次 seed 的等价性校验也不等于模型质量提升证据。

价格未配置时费用继续 unknown。验证平台是本机 `x86_64-pc-windows-gnu`；其他平台未据此宣称已运行验收。实际数据保护、备份和删除限制继续见 D 报告 §6。owner 日常使用验收不由技术门或本地 main 合入代替。

## 5. Main 合入与 MIT 发布输入收尾

- owner 指令“开始合入和收尾”后，将已验收 `a1fe0df` 快进合入本地 main；没有 squash、改写历史或丢弃分波提交。
- 原工作区六项许可证相关改动在切换 main 前后逐文件 SHA-256 一致；按独立文档与审计实现提交收口，项目 MIT 与第三方依赖自身条款分开维护。
- MIT/Cargo 元数据与审计源码的当前重验已经完成：18 个 workspace crate 的 Cargo metadata 均解析为 MIT；运行实现未改变，四波新证据、真实 C24 和 current-tree 回执均重新绑定当前发布输入。
- 2026-10-20 01:40:34（Asia/Shanghai），干净主线 `4c6f738934d2312ab4824320dac071b61e1968d9` 的完整 `verify-core-brain-closure.ps1 -Wave Final` 实际返回 **0**，末行为 `core-brain-closure (Final): PASS`。完整 workspace 测试为 **604 passed、0 failed、5 默认 ignored**；fmt、check、strict Clippy、compliance、S1-S99、真实 golden、A/B/C/D **47/50/36/71（共 204）** 项、四波封闭清单、typed verifier 和 current-tree core receipt 比对全部通过。
- 当前 core receipt 为 `sha256:8c2fbe9679dad28344cc3efe8dd3d1ca31999a05559d93365499ae7414c807b3`，其 outcome 为 `pass`；四波清单 digest 为 `sha256:ff13bf88ff28ecdea52b691ea429841200143b9df9afe24b5618c1429ffbe9a6`。C24 产品工件绑定 `2f65fcee4c0b9ba793fdc9045825ecd1af807e68`，grade 为 `LiveConfigured`，内容 digest 为 `sha256:2fd21a8e253d107251d5e6c0c87d683cae25c1bfccc0bcc727af05903d2e7465`；六类工作流各 2/2，费用仍为 unknown。
- 本轮一次先行 Final 在嵌套 D RuntimeOnly 复跑时被 core 审计拒绝，未计为通过；随后独立 D RuntimeOnly、core audit 和完整 Final 重跑均返回 0。日志仅保留在本机 OS temp，不作为便携证明。
- 本地模型配置仍只在忽略目录中；当前操作未创建安装包、GitHub release 或推送远端，未代替 owner 的日常使用验收。

## 6. 产品化分支当前源码的四波重采集（2026-10-27）

本节保留上文 main 的历史 Final，不把其回执复用到产品化分支。`shu/installed-product@da0d686e3806e0452d6687dfb0f67fd2f591891e` 的同一运行源码重新执行 A/B/C/D，逐名 **47/50/36/71** 项全部通过，18 crates/99 EventKinds 和前 97 strict prefix 保持。A 入口、B tick/相关性、C 六份 typed 工件及四份 runtime receipt 均重新生成并独立校验。

同源码 C24 正式 runner 使用本机忽略配置运行六类各两次，返回 `Pass (LiveConfigured)`；独立 `core_brain_release_audit.py --verify-product` 返回 `0`，产品 digest 为 `sha256:4e0ba7ec7ea83c83fbbc99cd8914417a6572242268b69be8d9e654d1b8b772af`。质量、直接模型质量基线和工作流每类均 2/2，不可验证为 0，费用仍 unknown。该短周期证据不证明长期收益或编排普遍优于直接模型。

四波工件与汇总清单已回填；`tools/core_brain_evidence.py --verify --require-pass` 返回 `0`，总清单 digest 为 `sha256:167c669b575333237064321304e8cb90c262bba346df9ad504f3fedc7f761e69`。A/B/C 保持 `ControlledRuntime`，D 为 `LiveConfigured`，不混淆等级。运行日志在本地 `target/installed-file-worker-evidence/closure-da0d686-{a,b,c,d}.log`，秘密未导出。

当前提交已完成 clean-tree core release audit：9 项检查全部 PASS，core receipt 为 `sha256:89294813276b54e6b161f67e8535e6cee35f0e3d8654e1e12670644d55c6de99`，独立 `--verify` 返回 `0`。M5 receipt 与完整 Final 是独立发布门，具体状态以 `m5-acceptance-report.md` 的当前记录为准；core audit 单独不提升为完整 Final。原生开发安装已恢复到 Host/Web 运行与未认领初始化页面，但 Brain/真实 provider/worker 与 IP01–IP24 继续独立验收，不据源码门推断。

## 7. 纠正 scope 与闭合清单后的完整源码终验（2026-11-09）

干净分支 `shu/closure-inventory@6899f5671ab137252e7b9851a58ca8ee869c09d5` 完整执行一次 `tools/verify-core-brain-closure.ps1 -Wave Final`。本地时间 **03:14:03–04:11:07**，实际 exit 0，终线为 `core-brain-closure (Final): PASS`，结束时 tracked/staged/untracked tree 均干净。原进程已经退出，没有另起完整 Final。

Final 的 workspace 阶段与其内嵌 M5 的 workspace 阶段各为 **183 targets、1310 passed / 0 failed / 16 默认 ignored**，分别从对应日志段统计，不叠加为新的测试总量。fmt/check、strict Clippy、Python **94 passed / 1 skipped**、完整合规、S1–S99 与 M4/M5 多进程 golden 均通过；M5 取得真实 `M5 FINAL ACCEPTANCE: PASS`，不是只生成审计回执。18 crates、99 EventKinds 和历史 97 strict prefix 保持。

A/B/C/D 在本次 Final 中逐名执行 **47/50/36/73，共 206 项**；包含两项新增 C23 测试和显式 ignored CLI。B/C typed 工件、四波闭合清单和 C24 当前源码/真实六类工件验证均通过。C24 复用本轮已实际采集的六类各两轮 `LiveConfigured` 原件；本次验证不冒充再次调用真实模型，也不证明长期收益或优于直接模型。

Final 内的生成/仓库回执独立验证与当前干净树 compare 全部 PASS。当次 core 为 `sha256:7bcc6fac780a1c1669f8dd527e4e283cdbbfc11ea140ee7086fb8c676c047069`，M5 为 `sha256:78eb9f2e29a19ddaac0abe638e082bdd1e871953b77b646a18a5acac4dabfb80`。它们对应上述 Final 源码；本节及其他验收说明随后提交时，按既有规则重新绑定规范发布回执，不扩大排除集，不把文档提交称为重新执行完整 Final。

原件位于本工作树 `target/closure-inventory-evidence/`：

| 工件 | SHA-256 |
| --- | --- |
| `current-final.log` | `4ab96146e99c5acafe843ea7b1d7a01323c1d74d7a3589812013aef3c4ceb080` |
| `current-final-process.json` | `db5dd711503a220ee1b6bb635590158c779f0859dd6e1cb9df03a703decd0e18` |
| `build-resources-after-final.json` | `81b82488e16f183af907bab2c5363c48c7fbf6c3f963e2306b13282e2838b87f` |

末次空间报告 PASS，缓存 **41,276,723,369 logical / 19,758,401,702 allocated bytes**，低于 64 GiB，`over_budget=[]`、`errors=[]`。本轮没有清理缓存、数据或证据。

本分支尚未合入 main，安装仍为 `cf3fbc9`，没有本轮管理员启动或升级回执。**本次关闭该源码快照的核心 Final 义务，不关闭整个安装产品目标。** 受管消费者、桌面授权、截图/引用输入/PTY 与其他执行 placement，以及逐项 IP 和平台证据，见[剩余义务审计](installed-product-remaining-obligations-audit-20261109.md)。该审计保留具体实现缺口和全部 IP01–IP24 的未验证状态。
