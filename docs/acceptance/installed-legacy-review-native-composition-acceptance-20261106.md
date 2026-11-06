# 原生历史审查生产组合：组件验收（2026-11-06）

协议提交 `b7fd4d0`，实现提交 `315960b`；分支 `shu/bootstrap-claim-journal`。本记录只证明当前源码与组件行为，不证明旧安装已有全部历史来源或原生恢复成功。

## 1. 实现

managed Host 通过独立 `ReleaseStagingConfig` 装配 `with_native_legacy_review`，固定读取受保护 `host_state/legacy-release-review/` 的 closed 布局。私有信任独立 pin 报告、来源审查、archive/origin 的摘要与公钥；五份有界原始证明不从浏览器导入。签名来源审查承担外部/人工审计的历史语义，runtime 不声称仅靠摘要证明过去事实。

原 operation 的 Planned 时间从既有 journal 重放取得；后续 RecoveryRequired/Aborted 不让保留的历史身份破坏 Host 重启，资格仍逐动作核查。收集器组合真实 archive reader、签名 management repair、原文件/SCM/3/3 能力检查、固定原数据 child，保持同一 worker/data/custody guard 到提交前。缺材料不可用，错材料拒绝，没有 fixture fallback、自动补签或恢复副作用。

额外修复普通 read custody 在读后新增 hardlink 未被重查的问题：记录原 handle/length，在摘要读取前后核对原件与链接数，复用已审计平台 adapter。无新 crate、依赖、EventKind、公开 DTO、冻结 trait 或 unsafe。

## 2. 实际检查

| 检查 | 结果 |
|---|---|
| `tools/verify-code-quality.ps1` | exit 0；182 Rust targets，1293 passed / 0 failed / 15 默认 ignored；fmt/check、strict Clippy、前端 115/115、依赖审计、bundle/signature/权限组件、原创性/许可证通过。 |
| 结构门 | 873 文件、63 处历史规模债务，无基线刷新；PASS。 |
| `tools/verify-installed-host.ps1` | exit 0；379 个逐名场景执行通过，另调用生产组合 suite 12/12，通过。 |
| 生产组合 12 项 | 正式 builder/原 Planned 时间/重放/材料签名/域/原字节 pin/closed/错身份/历史 schema null/worker-data-custody 漂移/真实 adapter 拒绝假 SCM 与 exe，均通过。 |
| 独立 Inno exit fixture | 已核对编译器 toolchain；成功 fixture exit 0、失败 fixture exit 1，2/2 PASS；没有安装 ForMe、启停服务或访问原脑。 |

完整质量门未传 Inno toolchain，所以 `quality.log` 保留其 NOT_RUN；独立补测由 `inno-exit.log` 证明，不改写原日志。不累计重复测试为更多独立场景。M5/发布回执没有由这些检查刷新，IP01–IP24 仍独立取证。

## 3. 工件摘要

以下文件位于本工作树 `target/native-legacy-review-20261106/`：

| 文件 | SHA-256 |
|---|---|
| `quality.log` | `39f277a547e0c0a8b717d7b57802e0467fd2834b820d8793fa9b6b12af3d0c7c` |
| `host-gate.log` | `b425d4437c6cf87c8bd2b4e4ee20e94dd1479222d34482b42b4bab2051fdd21b` |
| `inno-exit.log` | `e1af67bd011cc4ca0c181e6ce7782763c35c7417876daf61ef517dcfc918c677` |

原生旧操作的来源重建尝试和本机路线见[当前证据状态 §6](installed-legacy-recovery-evidence-status-20261106.md#6-本机路线与来源重现尝试)与[收尾方案](installed-product-completion-decision-20261106.md)。精确历史源码重建 exit 0，但与旧 Host 不同字节/摘要；不能算来源 Verified。

## 4. 边界

本批没有替换安装程序、修改原配置/ACL/journal、迁移数据、启停服务、重发原 Upgrade 或调用模型。原 Host/Web Running、Brain/updater Stopped；新 collector 未部署且历史材料未齐，原操作未裁决。Windows 浏览器检查因工具无法确认 URL 而停止，没有输入或页面变更。

原始数据 origin、历史 worker/lease 连续性、独立管理修复及旧操作收尾仍需真实材料或经明确审查的替代本机部署方案。生产接线通过、当前安装包验收、旧操作安全取消和整体目标完成是四种不同结论。
