# 截图结果 Host 组件验证（2026-11-09）

本批承接已批准的截图结果契约，实现原 job 的有界 PNG 接收、校验、Host 加密封存、descriptor 与重放。默认原生 helper 仍不支持输出；本批没有桌面捕获、core 材料登记、浏览器预览或部署证据，IP01–IP24 不变。

Host 在 Started 前持久绑定 schema 2 transfer，沿单一暂存槽限制并发和重启后的累计占用。Windows 保留原文件及目录祖先句柄；封存前校验所有 chunk、CRC、完整 PNG 解码及尺寸，再持授权锁重查原 grant、consent 与会话。AEAD 原件、manifest、descriptor 和暂存清理完成后才能接纳原 WorkerReceipt。失败保留 Unknown；重复原号不会再次捕获。旧 schema 1 journal 的序列化字节保持，descriptor 查询限 AgentExecution 域。

UTC 2026-11-09 13:57:26–14:30:57，fmt、workspace all-targets check/test、strict Clippy、installed-host 组件门、代码质量和完整合规七项均 exit 0。完整 Rust 回归为 191 个 test targets、1,418 passed、0 failed、22 默认 ignored；ignored 不计通过。新增场景覆盖原 PNG、CRC/解码/预算/顺序拒绝、暂存槽跨重启、Windows 文件保管、schema 1 前缀、schema 2 降级拒绝、封存时撤销及同号零重派。

本工作树 `target/product-integration-evidence/` 保留原件：

| 工件 | SHA-256 |
| --- | --- |
| desktop-output-host-gates.json | eb117d2ee1de6fcb2771f083bf6a05a32bc9cbcf8fb553ac5a7b5224bfed084a |
| desktop-output-host-test.log | e564d77af205f0126ba3bda6da138e90d8fb82e19eb96f8662937907ffac7605 |
| desktop-output-host-installed-host.log | 5dcd0180d7e4d60407c25bc0478ada3f5624cfff9610197c66e6a4ef116ead64 |
| desktop-output-host-compliance-final.log | 369aa4ca23bbb16cce58bb22be8be49e290675d13943609a496fa0cd0b6bdc92 |
| desktop-output-host-build-resources.json | ea71229d9b5f362863df02401c16c7f1fadafcecf77dd2a2c3b8447cfa24aa1b |

`verify-gate-receipt.py desktop-output-host` 独立逐项核验 996 个源码条目、全部七项日志及测试计数，PASS，结果在 `desktop-output-host-independent-verification.json`。结束时固定 target 逻辑 57,870,275,968 字节、实际分配 54,747,518,888 字节（约 51.0 GiB），低于 64 GiB 预算；末次 Cargo 前检查 198.1 GiB 可用。没有清空 target 或放宽资源门。

installed-host 明确报告 `IP01-IP24: NOT_RUN`。仍需完成 core 受保护登记、签名发布中的原生 v2 helper、认证工具产物目录/分块读取、浏览器原件摘要验证及实际安装联合验收；Host 执行域副本删除必须另取证。当前安装仍为 `forme-windows-x86_64-20261109-36f8662-dev`，不得用本源码回归代表已部署。
