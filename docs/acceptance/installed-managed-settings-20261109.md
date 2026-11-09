# 安装版受管设置第一批验证

本记录对应实现 `4dc1550`。owner 已批准三项协议补充；本批仅闭合设置保存、公钥装配和有限 Web 生态路由，尚未完成受管 MCP/插件/渠道的实际消费者、桌面使用流程或截图输出。它不是整体产品化完成回执。

## 实现范围

- 独立设置 schema 3 与 closed DTO；旧 schema 1/2 的读写语义保持，旧客户端保存保留新字段。
- Host 私有 schema 10 沿原配置事务持久化，publisher 公钥保存到受保护不可变材料文件。
- ProductionRuntime 在重建生态 registry 前装配并核验真实 Ed25519 公钥。
- Web 生态路由绑定认证 owner、原 plan 与原 nonce，拒绝浏览器自报审批 principal、路径/查询扩权。
- 没有真实 consumer 的 MCP/插件/Computer/渠道仍拒绝 enabled=true；保存 disabled 配置不启动发现或执行进程。

## 本机证据

`target/product-integration-evidence/managed-wave1-gates.json`：2026-11-09 UTC 10:06:25–10:25:30，结果 PASS。每项日志的 SHA-256 和验证前后源码摘要在该原件中；验证期间源码未变化。

| 检查 | 结果 |
| --- | --- |
| cargo fmt / workspace all-targets check | PASS |
| workspace all-targets test | 189 个测试汇总，1,369 passed / 0 failed / 22 ignored |
| strict Clippy | PASS |
| code-quality | PASS；911 files，63 项既存尺寸债务 |
| compliance-doctor | PASS |
| 全部产品二进制构建 | PASS |

两项新的 Web 生态测试在完整测试中实际运行；早先过滤器选中 0 tests 的命令不作为证据。忽略的原生/真实环境案例不因本批测试通过而计为已验收。

## 安装边界

本批没有替换现场安装。最近完成的现场恢复/升级仍见 [启动协调验证](installed-startup-reconciliation-20261109.md)，活动版本为 `forme-windows-x86_64-20261109-36f8662-dev`。后续能力启用必须由真实配置、运行消费者、网页流程及原生验收共同证明。
