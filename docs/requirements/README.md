# forme 需求文档目录

本目录存放 forme 的需求文档。它承接 `docs/planning/` 的规划结论，并为后续 `docs/architecture/` 技术方案和 `docs/prd/` 实施型 PRD 提供输入。

全局文档路线以 `../README.md` 和 `../documentation-map.md` 为准；本文只说明 `requirements/` 内部文档的角色和推进关系。

## 文档定位

需求文档回答这些问题：

- forme 为什么要做。
- 面向哪些用户和工作方式。
- 要解决哪些问题。
- 需要哪些能力域。
- 每个能力域的边界、优先级和验收方向是什么。

需求文档不做这些事：

- 不初始化 Rust 工程。
- 不定义 crate/API/数据库 schema。
- 不写具体实现步骤。
- 不直接复制或移植第三方项目能力。
- 不替代后续技术方案和实施型 PRD。

## 当前文档

| 文档 | 作用 |
|---|---|
| `01-vision-and-scope.md` | 第一份需求文档，固定 forme 的背景、定位、目标用户、要解决的问题、范围、非目标和关键约束。 |
| `02-capability-requirements.md` | 第二份需求文档，按能力域固定 M0-M4 的用户价值、边界和验收方向，并纳入 Communication Fabric、对外沟通、思想代理、Agent 间协作、时间化记忆、UserModel、ImportedHistoricalEvidence 和 remote executor 的阶段边界。 |
| `03-foundation-scope-decisions.md` | 第三份需求文档，固定 M0 是完整核心体系的第一条可运行闭环，并明确 CLI/Gateway、Communication Fabric、SQLite/FTS、MCP、skills、plugins、主动式、协调、认知地图、时间化记忆、UserModel、失败证据、delegation、approval 和合规边界的 M0 决策。 |
| `04-verification-and-acceptance-strategy.md` | 第四份需求文档，固定 M0 如何验收，覆盖 harness、gateway/communication fabric、persistence、memory/UserModel、MCP、skills、plugins、coordination、proactive、cognitive map、failure evidence、delegation、approval、CompetenceGate、并发一致性、撤销派生失效、schema replay 和原创合规检查。 |
| `05-m1-scope-decisions.md` | 第五份需求文档，固定 M1 是增强已闭合内核，范围为 Local Gateway 控制面、trace/manual eval、background/proactive workflow、context 和 capability 效率。 |
| `06-m1-verification-strategy.md` | 第六份需求文档，把 M1 转成 S23–S37 场景，并固定 S1–S22 继续作为不可回退的 M0 回归门。 |
| `07-m2-scope-decisions.md` | 第七份需求文档，冻结 M2 跨系统协作范围、A/B/C 波次、对外动作治理和 M3 非目标。 |
| `08-m2-verification-strategy.md` | 第八份需求文档，把 M2 转成 S38–S52，并固定 S1–S37 为永久回归门。 |
| `09-m3-scope-decisions.md` | 第九份需求文档，冻结 M3 受控自进化范围、A/B/C 波次、宪法边界、promotion/activation/authorization 分离和后续非目标。 |
| `10-m3-verification-strategy.md` | 第十份需求文档，把 M3 转成 S53–S69，并固定 S1–S52 为永久回归门。 |
| `11-m4-scope-decisions.md` | 第十一份需求文档，冻结 M4 单 Owner 联邦运行时范围、A/B/C 波次、peer/authority/lease/fencing/replication 边界和 M5 非目标；已落实。 |
| `12-m4-verification-strategy.md` | 第十二份需求文档，把 M4 转成 S70–S84，并固定 S1–S69 为永久回归门；owner 验收全绿并已封板。 |
| `13-m5-scope-decisions.md` | 第十三份需求文档，冻结 M5 受治理能力生态范围、D96-D115、A/B/C 波次和 package/publisher/authority 非目标；已落实。 |
| `14-m5-verification-strategy.md` | 第十四份需求文档，把 M5 转成 S85–S99，并固定 S1–S84、93-kind prefix 与 18-crate 图为永久回归门；S1-S99 已全绿。 |
| `15-v1-core-brain-closure-scope-decisions.md` | 冻结 V1 核心大脑运行闭合 B1–B24：唯一 composition、持续认知、长期项目/编排、多模型、数据治理与价值基线；明确不是 M6。 |
| `16-v1-core-brain-closure-verification-strategy.md` | 把 B1–B24 转成 C1–C24，并固定 S1–S99、97-kind prefix、18-crate 图和真实/重启/反向证据为永久门。 |

## 本目录推进关系

1. 先评审 `01-vision-and-scope.md`，确认愿景、目标、用户、范围和非目标。
2. 再评审 `02-capability-requirements.md`，确认能力域、M0-M4、用户价值和验收方向。
3. 再评审 `03-foundation-scope-decisions.md`，确认 M0 不是缩水 MVP，而是完整核心体系的第一条可运行闭环。
4. 再评审 `04-verification-and-acceptance-strategy.md`，确认后续架构和 PRD 必须如何证明 M0 没有跑偏。
5. 需求固定后，再进入 `docs/architecture/03-foundation-architecture.md`。
6. 架构方案固定后，再进入 `docs/prd/01-foundation-implementation-prd.md`。
7. M0 验收后，评审 `05`–`06` 冻结 M1 范围与验收，再进入 `architecture/04` 和 `prd/18`；不得直接从 planning 候选跳到实现。
8. M1 验收后，评审 `07`–`08` 冻结 M2 范围与 S38–S52，再进入 `architecture/05` 和 `prd/19`；A/B/C 每波验收后停下 owner review。
9. M2 验收后，评审 `09`–`10` 冻结 M3 范围与 S53–S69，再进入 `architecture/06` 和 `prd/20`；A/B/C 仍逐波停下 owner review。
10. M3 验收后，评审 `11`–`12` 与 canonical §26，冻结 M4 单 Owner 联邦边界与 S70–S84，再进入 `architecture/07` 和 `prd/21`；未通过规格 review 不写实现，每波继续停下 owner review。
11. M4 封板后，评审 `13`–`14` 与 canonical §27，冻结 M5 受治理能力生态与 S85–S99，再进入 `architecture/08` 和 `prd/22`；catalog、signature、admission 与 permission 必须保持分离。
12. M5 合入后，不创造 M6；评审 `15`–`16` 与 canonical §28，按 `architecture/09`、`prd/23` 的 A→B→C→D 把已有模块闭合为默认 forme 大脑，C1–C24 与 S1–S99 同时通过才算完成。
