# 受管 MCP 注册表读取边界验收（2026-11-10）

实现提交 `c1010a1d1b339b4d315b1dea00cf14e053440818`，落实 [stdio 契约 §7](../architecture/installed-managed-mcp-stdio-contract.md)。正式 managed composition 现在选择独立受管 registry 模式；缺 Core 目录来源即不可用，不能通过发现或刷新退回开发 stdio 进程。Core 真实原件读取器尚未装配，当前来源明确为 None；公开 enabled 保护保持，本批不是安装版 MCP 完成。

## 行为与边界

新增窄 ManagedMcpCatalogSource 接缝，不改变原 McpRegistry/ExecutionCapabilityRechecker 签名、Worker/Action DTO、99 EventKinds、18-crate 图或五个 unsafe 文件。Core 来源负责完整原事件链、受保护原件、当前注册/配置和删除 fence；registry 再核对 config/spec/catalog、grant scope、program_ref 与注册摘要映射、空 args/env、allowlist、schema 和预算。

目录读取、搜索、schema 选择、准备调用、索引和执行重查均重新读取来源并比较已装载摘要。失败或变更会移除该来源缓存；显式刷新也只能读取现有原件，不启动 MCP，不发布新的 McpDiscovered。工具和资源按精确 allowlist 过滤，schema 保持延迟选择。受管模式的读取与 provider 生命周期变更有排他顺序；开发模式继续使用原路径。

受管普通调用保留原 Mcp 参数形状，使用 opaque program_ref 和空 args/env，以 High/Outward 进入原 Harness。执行重查另拒绝程序、参数、环境、期限、作用域和风险/作用性质替换；Host 实际写入时仍独立核对权限及原 discovery。元数据不能授予执行权，registry 缓存也不能代替 Host 准入。

## 验证

2026-11-10 17:19–17:36（Asia/Shanghai），fmt、workspace/all-targets check、strict Clippy、完整测试、产品程序构建、原生 MCP、原结果、原 PTY、质量与完整合规十项全部 exit 0。整仓 **200 targets、1,527 passed / 0 failed / 32 默认 ignored**。

新增目标 **10/10**，包含九项行为场景和一个进程夹具入口：精确 allowlist/延迟 schema、六个缓存消费者的失效、schema 变化后旧计划拒绝、八种调用替换、十种来源/配置替换、禁用及缺信任、重建只读原件、无来源/错误来源时零本地启动，以及禁用和目录读取的并发顺序。开发 stdio 既有目标 **5/5** 保持。无 fallback 场景先实际启动自建子进程证明标记夹具有效，再验证受管缺源/失败路径均不生成标记。

测试来源是受控的 ManagedMcpCatalogSource，不等于已完成 Core 原件链或真实安装身份恢复。参数校验沿用现有 object/required 准入和 schema 身份检查，不声称新增完整 JSON Schema 验证器。初次定向测试的两处错误假设（直接绑定 Untrusted、期待属性类型校验）已按现有协议修正，原失败日志 `managed-registry-r1-test.log` 保留。

同一源码的原生 MCP **10/10**（24.76 秒）、原结果 **1/1**、PTY **4/4** 通过。PTY 11 份独立回执均 result/cleanup PASS、原 Job 空、installed_acceptance=NOT_RUN。独立核验检查 1,075 份源码、十份日志、原生二进制/fixture 和回执，提交后源码再次匹配；源码 manifest SHA-256 为 `2d532eeedc76892808f7cd4be052a592cb8f0353fe6046044ac0398dfb050248`。统计按每个 Cargo target 的最终汇总，不重复累加子进程夹具自身的 libtest 输出。

质量门为 **1,045 files、62 项既有规模债务**。原 mcp.rs 从 1,001 降至 976 行，production.rs 从 2,603 降至 2,598 行；新增生产文件为 199/69 行，测试 472 行，未刷新规模基线。

## 空间和可追溯原件

本批重新链接后固定 target 实际分配为 **71,744,444,289 bytes（66.82 GiB）**。全部构建/缓存进程退出、源码提交干净后，先预览再压缩 55 个 exe；逐文件 SHA-256、长度及 Git HEAD/status 保持。回收 **11,783,775,774 bytes（10.97 GiB）**，最终为 **59,960,668,515 bytes（55.84 GiB）**，空间报告 PASS、无错误、未超 64 GiB 预算。

这是本批重新构建后的独立维护窗口，不把它与[前批回收](installed-managed-mcp-material-20261110.md)简单相加作为最终净节省。没有删除源码、缓存、安装包或验收原件。下列工件位于本工作树 ignored `target/product-integration-evidence/`：

| 原件 | SHA-256 |
| --- | --- |
| `managed-registry-final-gates.json` | `c2921dc8f42ec667e3960fc8f1d39fb100350afd8e57b80985e21bb030d5d828` |
| `managed-registry-final-independent.json` | `ca4701c023f987c75f1c50138091d9329681e643596b39a267f23d0dc3695867` |
| `managed-registry-final-commit-binding.json` | `ff9ca5adf92c88241316a49d64c2520527c9fadd746f533c33de63797893485f` |
| `managed-registry-cache-compression.json` | `b5f7a5bf8d9a4d3b2a184b057afef5d53cc383349af4aeb5899f1e27e3ebef6c` |
| `managed-registry-after-compression-resources.json` | `a684edd8445edf12bc87de19e4f3f35b152a56f6efc7338dfb32ecb8aeaff950` |

## 剩余接线

Core 来源还须校验原 RunAccepted/SessionBound/ActionPlanned/Started/Completed，结合当前 Host 注册/配置和删除状态发布/恢复 metadata；不能只读本地加密 catalog 就使工具可用。owner 显式发现需进入原 Harness exact-plan 审批，并持久化用于恢复的原 command/spec；正式认证 Web 入口和设置 UI 随后接入。

插件、渠道、后续 profile、桌面联合验收与完整 IP01–IP24 继续按已批准范围完成。本批没有远程推送、部署、服务/UAC 操作，也没有刷新 M5/D/Final 或安装产品验收结论。
