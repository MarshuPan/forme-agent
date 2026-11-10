# 首次 MCP 发现装配组件（2026-11-10）

实现 `82128a494856654b7ab1d6ff49549dac4100223f` 落实 [stdio 契约 §9](../architecture/installed-managed-mcp-stdio-contract.md)。受管执行器现在可以在目录尚不存在时按有效配置装配；普通 MCP 工具仍须有完整原件与可用隔离 backend。此批解决装配层依赖循环，尚未接通 owner 发现入口，不能把它当作安装版 MCP 已可使用。

## 行为

- managed 模式始终只读核验原件，开发 discover_on_start=false 不能使无目录 provider 自动 active。暂缺原件时保留合法 provider 生命周期，后续可显式载入已有完成目录；读取不启动程序、不批准动作。
- disabled、非 OwnerInput、缺 grant、budget 不一致或无法解析的输入均不获得 worker 装配资格。若隔离 backend 缺失，即使目录可读，也不生成正式工具绑定。
- managed 程序引用不接受调用方 args、env 或 inherit_env，在读取 Brain 环境前拒绝。受管输出采用严格上限；普通受管 MCP 绑定使用 High，与 registry/Host 的调用规则一致。
- 原 policy、CompetenceGate、审批 nonce、Unknown 和 native 身份检查保持。公开 enabled 保护未取消。

## 验证

原整轮 fmt、workspace/all-targets check、strict Clippy、完整测试均 exit 0；A 波场景通过但封存因源码未提交而失败，原轮 outcome 保持 FAIL。随后同源码质量与合规通过，提交代码后使用独立新目录重跑 A 波，完成正式封存。独立报告核对三份回执、相同源码清单与全部日志摘要，不将原失败改为通过。整仓 **200 targets、1543 passed / 0 failed / 35 默认 ignored**；新增五项全部通过。A 波门逐名执行 **47** 个场景，并构建实际 CLI/Gateway；其 PASS 不表示安装服务 IP 通过。

新增场景覆盖首次无目录仍可准备 executor、关闭启动发现标志也必须有目录、后续完整原件无需重启 provider 即可读取且删除后拒绝、六类禁用/信任/授权/预算/来源拒绝，以及三类进程参数/环境拒绝。来源采用计数夹具，未伪称 Brain 服务身份或 Windows 原生完整发现已验收。

独立核验检查 1084 份源码、全部日志摘要、五个测试名与闭合门终线。源码清单 SHA-256 `9459c0b60bd4bc4a86c75c7aa8a98221d553b0c249dbe25e12551f82e3a50285`。质量门：`code-quality: PASS (1054 files, 62 existing size debts)`。production.rs 拆出目录准备职责，新生产模块 111 行、测试 248 行，原超限文件缩短，未刷新规模基线。

原件位于本工作树 ignored `target/product-integration-evidence/`：

| 原件 | SHA-256 |
| --- | --- |
| mcp-bootstrap-final-gates.json | 4df3a1523b689dca660a46f65a0c182f51d4459dfaf78e3dac2d19d3e30b98ac |
| mcp-bootstrap-precommit-tail.json | 42a7e5907a1c8c1026b6e394700e8c6ed6fba6e53facf7ee567dc515df913b77 |
| mcp-bootstrap-clean-a-gates.json | db1c4ad850bf09932e2da10ccc9b88790bb5d5628b86d245932115e32f0932bf |
| mcp-bootstrap-final-independent.json | d8f723315c4a6dafd2a4b2faf6678f3ba0c462a95716db80388bd6fae6038a72 |
| mcp-bootstrap-final-build-resources.json | 34037bae7a41554ca1deb5f025d9f14bfc55a62211435f05f80fa3a98771b870 |

本轮阶段末缓存实际分配 73,600,658,561 bytes；后续空闲维护和完整 M5 的回执另按实际结果记录于 [M5 验收报告](m5-acceptance-report.md)，不把多轮压缩简单相加作为净节省。

## 下一接缝

原三项提案和 discovery 动作的授权有效。owner 专用认证入口、配置映射、exact pending/不可变计划的保护与重启恢复、完成目录发布、动态 toolset 和 Web 接线仍开放。[显式发现能力门补充](../architecture/proposals/installed-owner-directed-discovery-competence.md)已由 owner 明确批准，下一批按限定范围实施；本批代码的治理尚未改变。

插件/渠道接入、桌面安装联合验收及 IP01–IP24 继续保持原范围。本批未部署、未请求 UAC、未修改系统服务或真实 owner 数据。
