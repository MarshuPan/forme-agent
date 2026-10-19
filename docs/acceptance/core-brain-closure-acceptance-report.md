# V1 核心大脑运行闭合：整体验收状态

日期：2026-10-19。分支：`shu/v1-core-brain-a`；`main` 仍为 `94fe6f1`。

**真实 C24 已通过，Final 总门仍在闭合。** 已在干净验收检出复跑 A/B/C/D runtime，并完成六类各两次真实模型试验与独立校验。四波清单和 current-tree 发布审计尚在封存，不能提前把 live runner PASS 写成 V1 Final PASS。

## 1. 各波与证据边界

| 波 | 内容 | 当前证据 |
|---|---|---|
| A：C1–C6 | 默认 ProductionRuntime、持久 owner、context、能力与统一入口。 | `025ad3d` 输入复跑 43 项 PASS；正式入口/共同快照见证已导出。 |
| B：C7–C12 | 三相 tick、AskToLearn、AgentWorkspace、扩散/直接模型基线、维护。 | 同输入复跑 48 项 PASS；三相 tick 与比较工件为受控 HTTP，不能作为线上质量证明。 |
| C：C13–C18 | Charter、八模式与六维匹配、route 治理、长期项目监管、经验回流。 | 同输入复跑 35 项 PASS；六份重新生成的 runtime/SQLite/files 工件独立验证。 |
| D：C19–C23 | 实际模型 attempt、看板、备份恢复、加密迁移与真实删除。 | 同输入复跑 70 项 runtime PASS；修复后全工作区 597 passed、0 failed、5 默认 ignored；fmt/check/strict Clippy/compliance PASS。 |
| C24 | 六类真实任务、基线、typed artifact 与独立 selector。 | **LiveConfigured / Pass**：六类各两次，质量、基线与工作流均 2/2；独立 verifier exit 0。 |
| Final | C1–C24、S1–S99、99-kind/97-prefix、18-crate graph、真实 golden、clean-tree release。 | **尚未通过**；待当前树四波清单和 core/M5 receipt 封存后实际运行总门。 |

历史 A/B/C 报告不回写当时事实。本轮具体提交、正反证明、命令、限制与待办见 `core-brain-d-acceptance-report.md`；操作步骤见 `../runbooks/v1-core-brain-data-and-evidence.md`。

## 2. 真实试验的失败、修复与边界

第一次真实试验 `1d1f671` 的工具类在真实写文件后未完成模型续接；原记录归档且保留 exit 2。`5ffcf74` 修复原生 assistant/tool_call_id 往返与审批恢复，`1a79b57` 加强 JSON 解码后隐私扫描，`025ad3d` 修正入口校验器把定时通知误作模型对话的口径。最终受理的一轮从 `025ad3d` 再采四波和真实 C24，当前产品内容 digest 为 `sha256:027af5b21114b0e4da85950e49fa6cbb8feaefbb2c094847ead9a1ee7b23e7b7`。

实际 profile 为 `compatible:gpt-6.1-sol`，配置与密钥未进入 Git/工件/报告。缺价格的费用保持 unknown。六类有界工作流不证明数月认知收益、编排普遍优于直接模型或全场景长期自治；A/B/C 的受控 HTTP 不因 C24 live 通过而升级。

接下来完成封闭四波与汇总清单、当前干净树 core/M5 发布 receipt 和 Final。历史 receipt 不替代当前树；owner 验收与 main 合入独立记录。

本轮没有修改 `main`，没有纳入原工作区独立的许可证修改。当前剩余的是最终发布通过证据；不是增加一个 M6，也不是再添加内核。
