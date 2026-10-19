# V1 核心大脑运行闭合：整体验收状态

日期：2026-10-19。分支：`shu/v1-core-brain-a`；`main` 仍为 `94fe6f1`。

**本轮七项默认大脑补闭已实现，四波运行证据及真实 C24 已通过；Final 正在重验和重封发布回执。** 当前受测源码是 `b9ea670accdae0f59122aa62333cb172e18416e4`。此前 `03d2159` 的 Final PASS 是历史事实，只绑定当时的 `025ad3d` 输入，不能替代本轮验收。owner 人工验收与 `main` 合入仍分别记录。

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

## 2. 四波和真实模型证据

| 波 | 本轮逐名执行 | 证据与边界 |
|---|---|---|
| A：C1–C6 | 47 项 PASS | 正式装配、共同入口、owner/workspace、context 与权限；entrypoints 见证为 ControlledRuntime。 |
| B：C7–C12 | 50 项 PASS | 三模型、实时能力、三相 tick、学习、扩散/直接模型对照和维护；保留扩散不如直接模型的受控负结果。 |
| C：C13–C18 | 36 项 PASS | 项目、八个编排种子、DAG 治理、监管与受治理经验；六份新生成的 SQLite/文件/事件工件由独立 verifier 通过。 |
| D：C19–C24 runtime | 71 项 PASS | 真实 profile、数据控制、备份、加密、删除、默认 M3 和工具审计；RuntimeOnly 本身不关闭 C24。 |
| C24 live | 六类各 2 次 PASS | 冷启动、工具、长期 workspace、多 child、AskToLearn、失败重协调，质量/直接模型质量基线/工作流均各 2/2；不可验证为 0。 |

C24 来自干净源码 `b9ea670`，正式 runner 返回 `Pass (LiveConfigured)`；独立 `forme-core-brain-d-artifact-verify --require-pass` 与 `core_brain_release_audit.py --verify-product` 均返回 0。产品内容 digest：`sha256:9e7c0a2a03cfee4f2fccb40754baa42383f1bf48acfdbc5a1777f55246ef647a`。

四波和汇总清单已更新：`core-brain-closure-artifacts/manifest.json`，digest：`sha256:b957358c7b639fdca4863de548ccd187a4fd7a55d9fdaedb3290f2c016f276db`；`tools/core_brain_evidence.py --verify --require-pass` 返回 0。所有运行 receipt 均绑定同一源码输入和当前 204 项冻结清单；工件保留事件位置、快照、内容摘要和隐私校验。A/B/C 不因 D 使用真实 provider 而升级为线上证据。

## 3. Final 与发布回执

本轮正在按下列完整命令重验；发布回执重封完成前，不将旧回执作为当前树证明：

```powershell
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_TARGET_DIR = Join-Path $pwd 'target'
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-core-brain-closure.ps1 -Wave Final
```

当前实现的 fmt/check/完整工作区测试/strict Clippy/compliance 已通过；完整 Final 还要求 S1–S99、真实 browser/API/evolution/federation/ecosystem golden、四波逐名执行、独立 typed verifier 和当前干净树 core/M5 receipt 重算一致。最终返回码与本轮回执在完成后更新本节。

此前 2026-10-19 13:05（Asia/Shanghai）`03d2159` 的完整 Final 返回 0，源码输入是 `025ad3d`，四波为 43/48/35/70 项。它与首次真实工具调用失败、修复后的历史通过结果均保留，不改写成当前源码证据；详细历史见 `core-brain-d-acceptance-report.md` §3–5。

## 4. 验证范围与实现限制

本轮保持 18 crates、99 EventKinds、前 97 exact prefix 与既定依赖方向。实现提交为 `b9ea670`；此前契约与操作文档单独提交，数据和验收报告另外提交。原工作区六项许可证相关既有修改逐文件 hash 保持不变；本机模型配置与密钥未进入 Git 或便携工件。

七项新增测试使用受控 HTTP、正式运行时、真实文件和 SQLite 重启，证明接线与治理。真实 C24 的六类短周期工作流另行验证；两者均不证明数月认知收益或编排普遍优于直接模型。当前项目规划仍接收类型化任务签名，路线经验采用受治理的成功/失败适配分数，不将其描述成通用自然语言到 DAG 推断、成本最优化或完整在线优化器。首次 seed 的等价性校验也不等于模型质量提升证据。

价格未配置时费用继续 unknown。验证平台是本机 `x86_64-pc-windows-gnu`；其他平台未据此宣称已运行验收。实际数据保护、备份和删除限制继续见 D 报告 §6。`main` 合入和 owner 人工验收不由技术门代替。
