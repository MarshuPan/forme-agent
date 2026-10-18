# V1 核心大脑运行闭合：整体验收状态

日期：2026-10-19。分支：`shu/v1-core-brain-a`；`main` 仍为 `94fe6f1`。

**当前不是 V1 Final PASS。** A/B/C 已有分支实现与整波自检；D 的 C19–C23 实现及 70 项运行场景通过，C24 runner/独立校验器已落地，但真实模型配置不可用，六类任务均没有实际试验。D 正式门与最终 release gate 必须继续拒绝通过。

## 1. 各波与证据边界

| 波 | 内容 | 当前证据 |
|---|---|---|
| A：C1–C6 | 默认 ProductionRuntime、持久 owner、context、能力与统一入口。 | 当前 D 源码复跑 43 项逐名场景 PASS。 |
| B：C7–C12 | 三相 tick、AskToLearn、AgentWorkspace、扩散/直接模型基线、维护。 | 当前源码复跑 48 项 PASS；B 工件明确受控 HTTP，不能作为线上质量证明。 |
| C：C13–C18 | Charter、八模式与六维匹配、route 治理、长期项目监管、经验回流。 | 当前源码复跑 35 项 PASS；六份受控模型/真实 runtime/SQLite/files 工件及新生成工件独立验证。 |
| D：C19–C23 | 实际模型 attempt、看板、备份恢复、加密迁移与真实删除。 | 70 项运行门 PASS；工作区 593 passed、0 failed、5 默认 ignored；尚需与当前干净树发布回归共同核验。 |
| C24 | 六类真实任务、基线、typed artifact 与独立 selector。 | **Unavailable / UNVERIFIABLE**；正式 runner 与 verifier 都 exit 2。 |
| Final | C1–C24、S1–S99、99-kind/97-prefix、18-crate graph、真实 golden、clean-tree release。 | **未通过**；不得由实现存在、受控测试或历史 receipt 推导完成。 |

历史 A/B/C 报告不回写当时事实。本轮具体提交、正反证明、命令、限制与待办见 `core-brain-d-acceptance-report.md`；操作步骤见 `../runbooks/v1-core-brain-data-and-evidence.md`。

## 2. 完成 Final 前必须成立

1. owner 在本机配置真实、支持工具调用的模型，凭证不进入 Git/工件/聊天。
2. 从已提交源码向新目录运行六任务 runner，每类至少两次；按真实失败修复，不能用受控响应补齐 live 结果。
3. 独立 verifier 和 source-commit 校验 PASS 后更新仓库产品工件，再跑 D 正式门。
4. 在干净检出、该 checkout 自己的 `target/debug` 下完成永久回归、current-tree release receipt、独立验证和 Final 门；旧 receipt 不替代当前树证明。
5. owner 验收与 main 合入单独记录，未合入的分支不称作正式基线。

本轮没有修改 `main`，没有纳入原工作区独立的许可证修改，也没有声称可打包日常使用。当前缺少的是 C24 真实运行和最终发布通过证据；它不是增加一个 M6，也不是要求再添加内核。
