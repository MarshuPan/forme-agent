# forme 规划设计目录

本目录存放仍在直接使用的规划设计文档。全局文档路线以 `../README.md` 和 `../documentation-map.md` 为准；本文只说明 `planning/` 内部文档的角色。

## 文档定位

规划设计文档回答“forme 要成为怎样的 Agent、某个内核能力应该如何组织”，但不替代需求确认，也不给出最终 crate/API/数据库 schema。规划中的候选能力必须先进入 `requirements/` 固定，才能成为必做项。

## 当前文档

| 文档 | 作用 | 使用方 |
|---|---|---|
| `15-multi-agent-orchestration.md` | 多 Agent 执行编排：确立“认知集中、执行分布”的单脑模型，子代理无独立认知、资源统一在大脑，编排库按适用签名匹配与沉淀。 | `requirements/15`、`architecture/09`、`prd/12`、canonical §13 |

## 已归档的早期规划

`01`–`14` 是 V2–V13 阶段的规划推导，结论已经收敛进 `requirements/`、`architecture/canonical-contract.md` 和 `architecture/02-03`，现存放在 `../archive/planning/`。它们只用于了解设计演进，同名对象和判断一律以 canonical 与 requirements 为准。
