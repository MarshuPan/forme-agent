# 受管模型代理配置验收

日期：2026-10-26。实现提交：`2702d30`；分支：`shu/installed-product`。设计基线为 `98ac262`。本记录覆盖受管模型代理配置这一组件，不提升 IP01–IP24，不宣布系统服务、安装包、真实外部 Provider 或完整独立部署产品已完成。

## 实现范围

- 新增版本化 `ProductProxyConfig`：支持 `direct` 与显式 `http`/`https` 代理两种模式。代理属于 Host 管理的产品配置，不进入脑事件、AgentExecution 或 Web 任意 URL 转发路径。
- Host 配置持久化升级到 schema 8。代理配置与模型、Brain、能力、网络配置共用 owner operation、CAS、revision、journal 和恢复语义；schema 7 的 release binding 保持兼容，发布/重启不会丢失代理配置。
- core materialization 将已验证代理写入 `model_proxy`；ProductionRuntime 在模型注册和 provider 装配时接收同一份代理快照。运行中的请求不会通过环境变量悄悄改换代理。
- 直接连接显式清空 curl proxy 并设置 `noproxy="*"`；受管 HTTP 代理显式设置 proxy 并清空 `NO_PROXY` 绕路。代理失败不会自动退回模型 origin，避免未授权的直连和隐式 fallback。
- 代理 endpoint 只接受有界的 HTTP authority：拒绝 userinfo、密码、路径、query、fragment、SOCKS、非法端口和控制字符。公共设置 view 只返回模式与 `endpoint_configured`，不返回 endpoint、凭据或 secret reference。
- 为保持维护边界，将模型传输实现拆到 `crates/models/src/transport.rs`，将协议对象拆到 `crates/protocol/src/product_config/proxy.rs`；没有新增内部 crate 依赖边，也没有把代理逻辑堆回产品配置入口。

## 验证结果

以下命令均在提交 `2702d30` 的工作树上通过：

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace --all-targets` | PASS，退出码 0，0 failed；Rust 默认 ignored 场景仍按测试语义未执行 |
| `py -3 tools/code-quality.py` | PASS，729 files，63 处既有规模债务未增长 |
| `py -3 tools/build_resources.py check` | PASS，约 249.8 GiB 可用空间 |
| `bash tools/compliance-doctor.sh` | PASS，upstream/license/copy 三项通过 |
| `git diff --check` | PASS |

专项回归覆盖模型传输、代理 endpoint 校验、直接/显式代理 curl 配置、代理失败不直连、公共设置脱敏、schema 8 持久化、release binding 保留和重启恢复。相关模型、协议、配置、release 与 ProductionRuntime 测试均包含在 workspace 全量回归中。

## 证据边界

- 本轮没有调用真实外部模型 Provider，也没有把真实 API key、proxy credential、`.forme` 数据或本地模型配置写入仓库、提交、验收文档或命令输出。
- 本轮没有注册/启动系统服务，没有执行真实 LAN/公网代理连通性验收，也没有执行 IP01–IP24。现有真实浏览器、Host、安装包和平台服务证据仍以各自专项记录为准。
- 因此，本记录证明的是“受管模型代理配置与模型传输边界已经实现并通过源码/fixture/回归门”，不是“任意真实 Provider 在任意代理网络下已经可用”。真实 Provider 的可用性仍需在用户自己的本地密钥和代理环境中单独验收。
