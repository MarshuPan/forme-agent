# 安装版桌面授权链路组件验证

实现提交 `7e4b7ce`，对应 owner 已批准的桌面授权补充与 [正式契约](../architecture/installed-desktop-authority-contract.md) §1–7。§8 的独立设置页正在随后一批实施；本批没有截图结果通道、受管 MCP/插件/渠道消费者或现场安装更新，不关闭三项提案的整体实施义务。

## 已接通的行为

- Host 独立持久化 pending、consent、grant、revoke 与消费事实；Windows 会话来自既有原生边界的实际枚举，Web 不提交 SID/会话号或执行 grant。
- owner 经认证、CSRF、原 operation 和密码重认证选择会话；Harness 仍重查原审批、最终 plan、policy、scope 与 capability，再取得绑定原 job 的一次性 grant。
- Computer 使用独立安装 placement。File profile 不提供桌面权利；配置禁用阻断新 pending、grant 与执行准入。旧配置保存与重启保留新状态。
- 浏览器刷新通过原 pending 查询 consent；查询失败不当作未授权的新请求。授权响应丢失只查询原 operation，原审批 nonce 在刷新后保持一致。
- 已核验的非截图 Committed 且 helper 无正文引用时，Host 封存真实零字节 AEAD 结果，以确定性 Host 结果引用绑定 Empty/Utf8 manifest 和 worker receipt。真实 Host、加密结果读取及 backend 完成链通过。声称有正文但没有传回时仍 Unknown；成功响应丢失后只 probe 原 job，helper 恰一次调用。

## 最终源码验证

`target/product-integration-evidence/desktop-authority-wave3-gates.json`，UTC 2026-11-09 11:54–12:14，PASS。891 份源码及每项日志 SHA-256 在回执中，另行独立逐份核对无不符。

| 检查 | 实际结果 |
| --- | --- |
| fmt、workspace all-targets check | PASS |
| workspace all-targets test | 189 suites，1,388 passed / 0 failed / 22 ignored |
| strict Clippy | PASS |
| 前端 TypeScript、行为测试 | PASS，123 tests |
| Web 资源可重复构建、全部产品程序构建 | PASS |
| code-quality、完整 compliance-doctor | PASS；940 files，63 项既存规模债务 |
| 构建资源 | 固定 target，最终回执空间检查 PASS |

最初真实 Host 空结果测试发现 Empty manifest 缺 result_ref，与 v1 既有验证约束不符；最终由 Host 对已封存的原空正文生成引用解决，没有放宽协议验证。失败日志 `desktop-result-fix-test-1.log` 与修复后 `desktop-result-fix-test-2.log`、包括响应丢失场景的完整 17 项 worker 合同 `desktop-result-fix-test-3.log` 均保留。较早 wave2 回执不用于证明后续空结果修复。

## 产品与浏览器组件证据

Ego Lite 中运行真实 Approval/DesktopConsent 与 WorkbenchClient，HTTP 由自建夹具提供。`desktop-browser-receipt.json` / `desktop-browser-state.json` 记录 consent POST 恰一次，刷新后审批 nonce 相同，查询失败禁用批准而保留拒绝。浏览器 TaskSpace 已结束；这不是原生桌面 driver 证据。

额外运行 `tools/verify-product-configuration.ps1`：87 项持久配置、网络 fence、脑偏好、受管 skills 与产品构建通过。四个真实进程场景中，配置/主脑 acknowledgement/重启、Web 单写者与本机身份边界三个先行通过；网络场景的旧断言仍认为 schema 3 不支持，得到 200 而预期 426。已改为核对真实 v3 managed/network 投影并拒绝未知 v4；仅重跑原失败网络场景，1 passed / 0 failed。原整门退出 1 保留在 `desktop-authority-product-configuration.log`，修正后的独立结果是 `desktop-authority-network-final.log`；不将原失败整门改写为 exit 0。最终工具改动后的质量、完整合规再次通过（`desktop-authority-precommit2-*`）。

这些进程使用临时目录与测试配置，没有注册真实服务、调用用户模型或控制用户桌面。本批没有刷新完整 M5/Final 或 IP01–IP24，没有推送远程或替换现场版本。安装版开关、截图原件与 Web 预览、实际消费者及原生服务/交互会话联合验收继续实施。
