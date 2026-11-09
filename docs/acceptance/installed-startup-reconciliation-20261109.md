# 2026-11-09 安装恢复与升级启动修复

源码提交：36f866230ca30874b549e67c2e42102d8a4437b1。整合 main、origin/main、closure-inventory 和 bootstrap-claim-journal 的祖先关系已在 shu/product-integration 保存；README 产品介绍保留。此报告只声明本轮已取证行为，不关闭所有 IP 或三个新增接线提案。

## 已取得的现场结果

- 用户之前明确允许重置测试安装。本轮只重置既定 ForMe 测试目录/服务；旧未完成升级不改写为成功。原始故障停在 stop_original_service/brain，原诊断和失败窗口保存。
- 新实例完成真实安装、认领、模型配置、自动启动、模型驱动文件拒绝/批准/读取/删除，以及停脑、离线备份核验、同一运行时重启。批准重放没有增加动作；篡改 plan 被拒绝。
- 原 da50343 → da50343-ip14 升级事务已提交，但启动时把原升级 Checking 当作普通激活失败，进程内失败标志未清除，导致服务运行正常却持续 Degraded。原失败验收回执保留。
- 36f8662 仅在目标 Host 启动且全部未完操作都具有持久原 Checking 原件时让普通激活让出；不发新服务动作、不改原 journal、不把 Checking 当作 Ready。其他未完/未知操作继续拒绝。
- 当前真实开发签名发布 `forme-windows-x86_64-20261109-36f8662-dev` 已通过正式 stage/upgrade 提交，product/core 均 Ready，issue_refs 为空，配置 3/ack 3。三个 SCM 服务 Running 且指向该发布；七个已安装二进制摘要与候选一致。
- 升级前后 owner/store/workspace 身份以及此前四个 run 的完整原事件数组一致。此包使用 dev profile，不是公开发行认证或跨平台结论。

## 验证

原回归先红后绿；38 项 release staging、15 项 startup/configuration 专项通过。完整 Windows 工作区：189 个测试汇总，1363 passed、0 failed、22 ignored；fmt/check/test/strict clippy、quality、compliance 和产品二进制构建均通过。

证据根为本工作树 `target/product-integration-evidence`。原安装/文件/备份窗口位于 closure-inventory 工作树的 `target/closure-inventory-evidence`。证据仅作本机取证，不提交密钥或认证材料。

| 原件 | SHA-256 |
| --- | --- |
| startup-gates.json | 2ec11367d5ad64089807ab2d73346170ab710d94217fee7ba6f231623757c5fc |
| startup-release-candidate.json | b3bb7c98b9efeafc24d5b64c77373b18304cfa748730890b69c8cd2c877fcd66 |
| startup-installed-upgrade.json | e7f36ba6ebcd5ec278312a33d9621298bdaa4bf60dd13417a789bf002f0921fb |
| startup-test.log | 5802c58e083ab190a1f87aec01c19c7a78db4302c959c969f545a3a00a1f3485 |

## 后续已授权范围

2026-11-09 owner 批准受管 MCP/插件/渠道、桌面授权和截图结果三项协议并实施，已解除相应冻结协议确认等待。按已批准范围接通真实消费者，再分别取得原生证据。通知 backend、桌面授权/输出、受管 MCP/插件/包/渠道、完整恢复及其他平台的未验义务不因本报告而消失。
