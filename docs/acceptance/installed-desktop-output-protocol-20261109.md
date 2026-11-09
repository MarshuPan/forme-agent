# 截图结果协议组件验证（2026-11-09）

owner 已批准截图结果方案。本次源码 `aca7837` 仅实现独立 schema 2 的绑定、transfer/header/chunk/finish/descriptor 与确定性 `WorkerArtifactBinding` 身份。原 desktop/session/worker schema 1 保持。它不捕获或传输 PNG，不构成原生截图、Web 预览或 IP11/IP17/IP24 验收。

校验包括完整 job/permit/owner/action/scope/plan/nonce/grant/session/generation、允许晚于 permit 的 grant、原 deadline、结果产出时间、64 MiB/像素/32 KiB chunk 限额、连续 offset、原始内容摘要及 closed metadata。协议测试中的字节专用于元数据验证，不能当作 PNG 解码证据。

本次门在 UTC 2026-11-09 13:23:31–13:45:45 完成全部六项：fmt、workspace all-targets check/test、strict Clippy、code quality、完整 compliance。191 个 Rust test targets：1,401 passed、0 failed、22 默认 ignored。忽略项不计通过。此前桌面设置的前端与进程验证没有在本批重复；本批没有修改这些表面。

原始回执在本工作树 `target/product-integration-evidence/`：

| 工件 | SHA-256 |
| --- | --- |
| desktop-output-protocol-gates.json | 608f98cb96b55b05b64334da3a2043d7d715326a497d2a410fcffe0d6e8c4275 |
| desktop-output-protocol-test.log | 591970f4a16d945629ce1e658f8a7a03d035c381d94f881dc8caa92b740e6573 |
| desktop-output-protocol-build-resources.json | e285b0e092248b0c3ef8a3d3217c396434452ca65d4f197b8af26578c13039a0 |

独立核验逐项比较了 983 个源码条目、当前清单、全部检查日志和测试计数，结果 PASS；原件为 `desktop-output-protocol-independent-verification.json`。固定 target 逻辑 33,884,656,108 字节、实际分配 30,765,883,077 字节，低于 64 GiB 预算，末次 Cargo 前空间检查为 221.1 GiB 可用。

后续继续 Host 暂存/原件验证/封存与恢复、原生 v2 helper、core 受保护材料及认证 Web 目录/预览。没有部署或替换当前安装服务，没有启用不完整截图能力；受管 MCP/插件/渠道等既有剩余义务保持。
