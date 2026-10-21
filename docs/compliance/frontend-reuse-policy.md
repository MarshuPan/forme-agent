# 前端限定复用规则

## 1. 授权范围与状态

owner 已明确允许借用 DeepSeek Harness 的页面结构、可复用前端组件和交互对话能力，同时排除其核心、插件化设计及运行时。本文件把这项**限定授权**落实为 canonical §12/§29、AGENTS 与 prd/17 的具体例外。

**当前工作台已采用 forme 自有前端实现，没有导入 DeepSeek 源文件、资源或业务逻辑，也没有复用文件白名单。** React/Vite 等实际依赖与许可证另记于 third-party-dependencies、根 NOTICE 和生成的 Web inventory。候选来源不等于已收录组件。未来导入仍须先满足本文的精确清单和许可证检查；这不是再要求一次 owner 确认，而是落实已授权范围的构建条件。

## 2. 候选来源

| 项目 | 公开来源 | 已知信息 | 使用方式 |
|---|---|---|---|
| DeepSeek Harness | [repository](https://github.com/deepseek-ai/deepseek-harness)、[LICENSE](https://github.com/deepseek-ai/deepseek-harness/blob/master/LICENSE)、[third-party notices](https://github.com/deepseek-ai/deepseek-harness/blob/master/THIRD_PARTY_NOTICES.md) | 仓库声明 MIT，web 使用 React/Vite；shell 与宿主/RPC 有耦合。 | 页面结构参考；仅对能独立拆出的组件逐文件审查。master 链接用于定位，实施时必须固定实际 commit。 |

来源清单须审查具体 revision 的文件头、嵌入资源和传递依赖，不能把仓库根 MIT 当作所有 icon/font/vendor 文件的许可证。公开 UI 截图不是源代码、品牌或素材的通用授权。

## 3. 可进入与不得进入的内容

**可列入白名单**：布局壳、通用按钮/输入/面板、可分离的消息显示与 composer、对话流/进度的纯展示交互。需删除上游服务/RPC 耦合并接入 forme DTO；不是简单替换 endpoint 后沿用其业务逻辑。

**不得进入**：Agent/Harness 核心、模型路由、记忆/认知、编排/loop、服务端、plugin/module runtime、Cordis 宿主、认证/授权实现、上游 prompts/错误文案/测试 fixture/命名目录体系、私有配置和凭据。组件若只能依赖这些才工作，就在 forme 中原创实现该视图。

上游整个目录不能被 blanket allowlist；`research/upstreams/**` 仍不得 build/import/include。已审查文件若需 vendoring，进入单独受清单管理的 frontend vendor 区并保留来源/许可标记，不复制上游工程组织结构。

## 4. 必须交付的 provenance 清单

每个复用单元的记录至少包含：

- 完整 source URL、不可变 commit SHA、原始文件路径、原文件 SHA-256。
- forme 目标路径、导入后摘要、改动说明、维护 owner 与使用目的。
- 实际 SPDX/license text/版权归属、传递依赖、资源许可与 notice action。
- `copy_risk`、允许的 UI 范围、排除的上游功能与解耦验证。
- 负责 reviewer、审查结果、发布 notice/SBOM 对应项。

合规工具只对记录中 exact source/destination/revision/hash 的许可 UI 内容做明确分类；它仍扫描未声明复制、跨范围变更和缺失语料。新增文件/上游更新/原文摘要变更必须重新审查，不能复用过期 allowlist。

## 5. 构建与发布门

1. 检查 registry 中的 source/license/notice 与实际构建文件一致；缺来源、缺许可或边界不明 fail closed。
2. 依赖图/bundle 检查无 Cordis、上游服务端/插件 runtime、隐式 RPC 或 provider credential；不能通过 tree-shaking 后“可能没执行”豁免。
3. frontend/package lock、传递依赖、renderer/icon/font 等形成 SBOM；新增实际内容才更新 third-party-dependencies/根 NOTICE。
4. 保留完整适用许可和版权 notices；白名单不覆盖测试 fixture、prompt 或后端。
5. `compliance-doctor` 与产品 release audit 均绑定白名单、上游语料、release tree 和 UI digest；不能关闭 copy detection 或扩大 GPL/未知许可接受范围。

当前合规脚本按现有原创边界扫描自有前端，编译生成物另由 Web 构建器校验来源摘要、exact lock、许可证与文件集合，不对手写源码增加复制豁免。正式导入前，须先实现精确许可复用记录与 fixture 校验，再导入许可文件；不得先复制再用全局忽略让检查变绿。当前结果不构成 DeepSeek 组件复用审查通过或已完成全部产品验收的声明。

## 6. 与原创性规则的关系

本例外仅覆盖 owner 指定的前端展示/对话交互，未列入清单的全部代码继续适用 canonical §12、prd/17 和 AGENTS 的不复制规则。forme 内核与所有业务/治理实现继续原创，所有测试 fixture 自建。

OpenClaw、Hermes、Muse、Grok 等仅作运行形态机制参考；它们不因本文而获得源码复用许可。后续实施中增加其他来源或扩大到核心/插件，需要新的明确范围决策，不能类推本例外。
