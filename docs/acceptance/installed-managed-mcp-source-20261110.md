# Core MCP 当前来源校验与首次装配边界（2026-11-10）

`ed2e20d` 实现 [stdio 契约 §8](../architecture/installed-managed-mcp-stdio-contract.md)：Core 只接受原 owner/run/session/composition、审批、plan、Started/Completed、保护原件及已发布 McpDiscovered 一致的目录；Host 只读验证当前注册/配置及原 worker admission 次序。较新 pending/Unknown、撤销、来源漂移和删除 fence 都阻断旧目录。读取不发起发现、不写新的脑事实，不把外部结果提升为可信指令。

## 组件证据

十项门均通过：fmt、workspace/all-targets check、strict Clippy、完整测试、产品构建、原生 MCP、原文件结果、原 PTY、质量与完整合规。整仓 **200 targets、1,538 passed / 0 failed / 35 默认 ignored**；11 项新增常规场景逐名核验。原生 MCP **13/13**（含 3 项当前来源场景）、原结果 **1/1**、PTY **4/4**（11 份原件）通过。1,082 份源码及日志/回执独立核验一致。

先行第二轮整仓发生一次普通 MCP 非读取服务器的两秒期限断言失败；三次独立诊断和最终整仓未复现，未放宽该断言，也未确定根因。`mcp-source-final2-test.log` 与三份 timeout diagnostic 原件保留；最终通过不抹除前次失败。

## 完整 M5 的实际结果

干净 `ed2e20d` 于 10:46–11:13 UTC 执行完整 M5。S1–S99 场景、进程 golden、fmt/check/Clippy、完整 Rust 测试、Python 95 项（1 skipped）及合规均通过；workspace 仍是 200 targets / 1,538 passed / 35 ignored。

最后比较步骤失败：新生成审计摘要为 `68e6d711bd251ae91f01ca694c53f908239106d02160b361051d1a538fc1f92b`，仓库仍保存 `694a2f92ebad40bec17d263b1347f4296b3e8ada2c8ef1dded448b5c79692837`。两份独立验证均通过但不属于同一源码树，**本次完整命令 exit 1，不能记为 M5 FINAL PASS**。原脚本 finally 清理了临时生成目录；生成摘要和失败留在原日志，新源码的发布回执仍需按正式审计器重建/比较。没有复用旧回执冒充当前树。

## 缓存维护

完整回归后固定 target 实际分配 75,586,881,515 bytes。确认无构建/缓存程序占用、源码干净后，先预览再可逆压缩 72 个 exe；逐份长度和 SHA-256 保持。回收 **15,505,348,390 bytes（14.44 GiB）**，最终 **60,081,533,125 bytes（55.96 GiB）**，低于 64 GiB 预算，无报告错误。未删除源码、缓存、安装包或验收材料；后续重新链接可能使部分缓存重新增长。

原件在本工作树 ignored `target/product-integration-evidence/`：

| 原件 | SHA-256 |
| --- | --- |
| mcp-source-final3-gates.json | 9b7240ac1aba1f33a859799809a20903b7b40e55d5b0d3e66d33540ff1a66fce |
| mcp-source-final3-independent.json | 10c30996d15266f38ba21eff2f5c032fedc4ee406ab615e1c227555873b1d47c |
| mcp-source-permanent-m5.json | ac4d1bf41f00dc3e03157a46dcd6e18922bd8ac887dce563297c38820ce7131d |
| mcp-source-cache-compression.json | 3640e34e6c753ddf98b8781ebda484f0875849ea487b99fda2b0e86f6622e1b3 |
| mcp-source-after-compression-resources.json | 4dc9dd06320bf67b4095ce4588d95509ef77fd70f72e85adfd1c5698bdeefb90 |

## 尚未关闭

原三项提案与发现动作的批准有效。首次装配不能依赖已有目录，严格输出与正式工具风险还需一致；owner 入口、待审批原计划的保护/重启恢复、完成发布、动态 toolset、Web 及公开启用继续实施。[首次装配契约 §9](../architecture/installed-managed-mcp-stdio-contract.md) 先固定这些独立准备行为。

另发现生产能力门最高 L4/高风险最高 L3，而执行要求 L5；[显式 owner 发现的窄治理补充](../architecture/proposals/installed-owner-directed-discovery-competence.md) 待确认，现有能力门保持。这是新发现的治理语义冲突，不撤销或重复请求原三项授权。

本机只读观察 Host/Web/Brain Running、updater Stopped；没有运行新的升级/UAC 或替换已安装版本。本组件、M5 源码门与完整 IP01–IP24 仍分开记录，插件/渠道和桌面安装联合验收继续开放。
