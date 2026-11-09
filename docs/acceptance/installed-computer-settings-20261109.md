# 安装版独立桌面设置验证

实现提交 `84e0406`，承接已批准的[桌面授权契约 §8](../architecture/installed-desktop-authority-contract.md#8-独立桌面设置与旧配置保护)。本批完成独立桌面开关及原配置保存回执查询；截图传输、受管 MCP/插件/渠道消费者和原生联合验收继续实施。

## 产品行为

- Web 认证、CSRF、原 operation 与密码重新认证绑定完整窄配置摘要，Host 沿原配置 journal/CAS 发布；只修改桌面配置，保留模型、网络、skills、publisher 公钥及其他受管字段。
- 缺安装 placement 时拒绝启用；禁用不申请动作授权。配置保存与运行生效分开显示，桌面动作继续使用原 pending/consent/grant 和审批流程。
- 保存响应丢失只查询原配置 operation，校验 owner、原摘要与 committed，再读当前配置；查询不能借维护 journal 的同名操作证明保存成功，也不自动重发 POST。
- 原号查询有界重读持锁配置日志、核对 sequence/head 并恢复 cursor；原件缺失或变化使查询拒绝且 writer poisoned，不能用内存投影制造已提交回执。
- 修正桌面后端测试对 Host 已封存空结果的旧预期；并行 Harness 夹具的日志名增加原子序号，避免仅依赖 Windows 时钟精度。保留原撤权、审批 nonce 和恰一次执行断言。

## 验证与原件

`target/product-integration-evidence/computer-settings-final4-gates.json`，UTC 2026-11-09 13:00–13:21，11 项检查全部 PASS。验证前后源码一致；独立逐份核验 980 项源码记录及全部检查日志无不符，结果保存在 `computer-settings-final4-independent-verification.json`。

| 检查 | 结果 |
| --- | --- |
| fmt / workspace all-targets check / test | PASS；190 suites，1,394 passed / 0 failed / 22 ignored |
| strict Clippy、代码质量、完整合规 | PASS；949 files，63 项既存规模债务 |
| 前端 TypeScript、行为测试、可重复资源构建 | PASS；126 tests |
| 产品配置门 | PASS；89 项持久配置测试及网络、脑偏好、受管 skills 检查；4 个真实进程场景全部通过 |
| 全部产品二进制构建 | PASS |
| 固定 target 空间报告 | PASS；逻辑约 31.5 GiB，分配约 28.6 GiB，低于 64 GiB 预算 |

真实 Web→Host 场景验证保存、同号重放、原摘要查询、Host 重启后同一回执，以及模型/网络保持。此前 Ego Lite 中真实组件与客户端的开关和丢响应恢复证据保存在 `computer-settings-browser-receipt.json` / `computer-settings-browser-state.json`，HTTP 由自建夹具提供；不作为原生桌面控制证据。

最终门回执 SHA-256：`d57c9fb4088ac0a2bcfa9a0d42a81ec504ea88aba54db880afbb49e1bc915aa8`。工作区测试日志：`078402218eb09d02e2d8328d3d884332cb721c6474629c96c0e99d4cf8d46b77`。原 final/final2/final3 失败回执和日志保留，不改写为成功；本记录只使用 final4 证明本批最终源码。

本批未替换现场安装、未推送远程、未刷新完整 M5/Final，也不关闭 IP01–IP24。最近已取证的现场版本仍按[启动协调验证](installed-startup-reconciliation-20261109.md)记录；后续消费者和安装包必须分别取得新的原生证据。
