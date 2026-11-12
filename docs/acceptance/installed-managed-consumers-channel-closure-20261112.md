# 受管消费者、工具快照与渠道组件闭合记录（2026-11-12）

本记录对应已批准的受管 MCP／插件／渠道接入、工具快照恢复和有限渠道出站方案。它记录源码与组件级证据，不把组件测试、安装包回执或本机服务观察提升为 IP01–IP24 的安装联合验收。

## 已接通的范围

- Run-bound 工具快照与恢复：快照绑定 run、schema、配置摘要和来源，重启读取原件；来源缺失、配置漂移、删除 fence、Unknown 或撤销都会阻断执行，不重新派发。
- 受管 MCP 设置和 Host 启用：配置、来源、凭据引用、程序注册和公开启用走独立的认证设置域；正式工具只在当前来源、能力、权限、审批和 Host admission 同时满足时可见。
- 插槽插件：插件来源、slot、别名和运行时消费者使用版本化注册；插件只能贡献声明和候选，禁用、撤销、来源丢失或运行时失败不会留下幽灵工具。
- 渠道接收：认证的本地渠道按当前 owner、workspace、source catalog 和 credential digest 重查后进入既有 Communication/Harness ingress；重复消息和重启沿原消息身份去重。
- 渠道出站组件：已接入 `ManagedChannelDeliveryGrantV1`、独立 channel AgentExecution 路由、Host intent、Low AppContainer/Job 内的有限 worker、固定 loopback POST、签名回执和 Unknown/no-retry 语义。发送内容来自既有 disclosure/envelope 与受保护内容引用，worker 不获得地址、凭据或任意网络能力。

## 证据

- `target/product-integration-evidence/remaining-quality-r4.json`：质量门 PASS，包含 fmt、workspace check/test、strict Clippy、代码质量、前端验证、bundle、原生包输入和合规检查。
- `target/product-integration-evidence/channel-workspace-check-r2.log`、`channel-host-check-r3.log`、`remaining-tool-slots-tests.log`、`remaining-plugin-runtime-slots.log`、`remaining-channels-check.log`：协议、Host、工具 slot、插件运行时和渠道组件回归记录。
- `crates/execution/src/installed/channel_delivery/tests.rs`：真实本地 TCP listener、Ed25519 回执、nonce 重启持久化、错误回执、断开和 redirect 场景；测试只使用本地 fixture。
- `crates/execution/src/installed/channel_delivery/tests/native.rs`：显式环境变量下的 `forme-file-worker` 有限 stdio worker 测试，默认 ignored；它是 Host 组件证据，不是已安装服务证据。

## 当前边界

1. 出站方案只允许已注册的本地 loopback fixture。当前没有真实联系人、第三方 provider、LAN endpoint 或凭据代理被调用；HTTP 外部出站的正式入口仍需通过既有 owner-authenticated Communication/Harness API，不能由 Web 或模型直接绕过。
2. 还没有取得真实安装环境的 SCM/service identity、AppContainer、UAC、浏览器工作台和签名候选联合证据。因此 IP01–IP24、Final、M5 release audit 仍保持未完成。
3. `remaining-m5-r1.json` 曾因工作树存在待提交改动而返回 `RELEASE_TREE_DIRTY`；代码与文档提交后，`remaining-m5-r2.json` 已完成功能阶段、完整回归、合规和 clean-tree S99，规范目录中的当前唯一 M5 回执已通过独立验证与 compare。
4. 固定 `target` 当前约 100.10 GiB logical、76.49 GiB allocated，超过 64 GiB 缓存预算。证据和运行时数据尚未删除；后续只按 `build_resources.py` 先预览再清理。

## 后续验收顺序

installed Host gate 仍需在当前提交上完成；随后在有真实安装身份和受控本机证据的环境执行联合验收。任何未知出站结果继续保留 Unknown，不重试或伪造 Delivered。
