# V1 核心大脑运行闭合：整体验收状态

日期：2026-10-19。分支：`shu/v1-core-brain-a`；`main` 仍为 `94fe6f1`。

**V1 Final 分支技术验收 PASS。** 2026-10-19 13:05（Asia/Shanghai），干净验收检出 `03d2159` 的完整 `verify-core-brain-closure.ps1 -Wave Final` 实际返回 0。A–D、六类各两次真实 C24、S1–S99、四波封闭清单和 current-tree core/M5 发布回执均通过。owner 人工验收与 `main` 合入是两个单独状态；本轮没有操作 `main`。

## 1. 各波与证据边界

| 波 | 内容 | 当前证据 |
|---|---|---|
| A：C1–C6 | 默认 ProductionRuntime、持久 owner、context、能力与统一入口。 | `025ad3d` 输入复跑 43 项 PASS；正式入口/共同快照见证已导出。 |
| B：C7–C12 | 三相 tick、AskToLearn、AgentWorkspace、扩散/直接模型基线、维护。 | 同输入复跑 48 项 PASS；三相 tick 与比较工件为受控 HTTP，不能作为线上质量证明。 |
| C：C13–C18 | Charter、八模式与六维匹配、route 治理、长期项目监管、经验回流。 | 同输入复跑 35 项 PASS；六份重新生成的 runtime/SQLite/files 工件独立验证。 |
| D：C19–C23 | 实际模型 attempt、看板、备份恢复、加密迁移与真实删除。 | 同输入复跑 70 项 runtime PASS；修复后全工作区 597 passed、0 failed、5 默认 ignored；fmt/check/strict Clippy/compliance PASS。 |
| C24 | 六类真实任务、基线、typed artifact 与独立 selector。 | **LiveConfigured / Pass**：六类各两次，质量、基线与工作流均 2/2；独立 verifier exit 0。 |
| Final | C1–C24、S1–S99、99-kind/97-prefix、18-crate graph、真实 golden、clean-tree release。 | **PASS**；完整命令 exit 0，末行为 `core-brain-closure (Final): PASS`，当前 core receipt 独立校验与重算一致。 |

历史 A/B/C 报告不回写当时事实。本轮具体提交、正反证明、命令、限制与待办见 `core-brain-d-acceptance-report.md`；操作步骤见 `../runbooks/v1-core-brain-data-and-evidence.md`。

## 2. 真实试验的失败、修复与边界

第一次真实试验 `1d1f671` 的工具类在真实写文件后未完成模型续接；原记录归档且保留 exit 2。`5ffcf74` 修复原生 assistant/tool_call_id 往返与审批恢复，`1a79b57` 加强 JSON 解码后隐私扫描，`025ad3d` 修正入口校验器把定时通知误作模型对话的口径。最终受理的一轮从 `025ad3d` 再采四波和真实 C24，当前产品内容 digest 为 `sha256:027af5b21114b0e4da85950e49fa6cbb8feaefbb2c094847ead9a1ee7b23e7b7`。

实际 profile 为 `compatible:gpt-6.1-sol`，配置与密钥未进入 Git/工件/报告。缺价格的费用保持 unknown。六类有界工作流不证明数月认知收益、编排普遍优于直接模型或全场景长期自治；A/B/C 的受控 HTTP 不因 C24 live 通过而升级。

四波与汇总清单已封存：`core-brain-closure-artifacts/manifest.json`，digest 为 `sha256:fd30e7ba6fd4b3873824f8a8ebb6764db055c1d049310f9c6b3dd844ae5be816`。A/B/C 的证据等级继续为 ControlledRuntime，D 产品为 LiveConfigured；所有波均绑定共同源码输入、文件摘要、事件范围、快照和隐私扫描。

## 3. 最终门与发布证据

```powershell
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_TARGET_DIR = Join-Path $pwd 'target'
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-core-brain-closure.ps1 -Wave Final
```

总门实际完成完整 Rust 回归、strict Clippy、65 项 Python 工具门（64 通过、1 项权限相关跳过）、S1–S99、真实浏览器/API/演进/联邦/生态 golden、四波 196 项逐名场景与全部独立工件校验。5 个默认忽略的 Rust case 在相应正式场景门中显式运行，不能按默认忽略数推导覆盖。

core current-tree release receipt：`sha256:ad82603edab268efef0bf7b712738ae11201b60a1c57c01df04cf0a5ae68aaae`，目录 `core-brain-closure-release-audit-artifacts/`。M5 current-tree receipt 位于 `m5-release-audit-artifacts/`，随报告收口重新生成；不将其摘要写入受它约束的报告，避免循环。core 明确排除两个当前报告与 M5 receipt；报告收口不改变受测源码，仍须校验当前根摘要和封闭清单。

完整日志 `forme-v1-final-gate-025ad3d-complete.log` 保留在本机 OS temp；便携证据为已提交清单与 typed artifacts，非本机日志路径。报告变更后只刷新受影响发布回执并复核，不把旧 receipt 当当前树证明。旧 current M5 receipt 的替换可从 Git 历史恢复。

本轮验证目标是 `x86_64-pc-windows-gnu`；非本目标的依赖告警保留 observation，其他平台未据此宣称验收。原工作区六项既有修改逐文件 hash 保持不变；本机模型配置不进入 Git。当前剩余状态是 owner 人工验收与 `main` 合入，不存在未完成的 A–D 代码/自动门；它不是 M6。
