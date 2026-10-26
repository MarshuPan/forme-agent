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

## Web 设置接入（2026-10-26）

实现提交：`e817534`。本批将已有的受管代理配置接入 Web 设置页，未改变 Host、模型传输、权限或产品配置协议。

- 设置页支持 `direct` 与 `http` 两种模式；HTTP 地址只写入请求，不从公共配置 view 回显。
- 已配置代理但未重新输入地址时，前端保持原配置；切换为 HTTP 后必须输入完整地址，避免误清空或提交不完整配置。
- 前端解析器拒绝缺失/错误的代理公开投影，也拒绝任何意外回显的 `endpoint` 字段。
- 保存请求沿用原有 owner operation、revision 和失败恢复绑定，并只在用户实际修改代理时发送 proxy patch。
- 重新生成并同步内嵌 Web bundle，避免源前端与 `forme-gateway` 内嵌资源漂移。

## 本批验证

| 检查 | 结果 |
|---|---|
| `npm run verify --prefix frontend` | PASS，TypeScript、89 项前端测试、Vite 构建和内嵌 bundle 核对通过 |
| `pwsh -NoProfile -File tools/verify-code-quality.ps1` | PASS；Rust workspace、strict Clippy、前端、原生打包静态门、合规门和 diff 检查通过 |
| `git diff --check` | PASS |
| 代码提交 | `e817534`，`feat(web): expose managed model proxy settings` |

质量门报告的构建缓存预算提示为警告，不影响本批通过；原生安装/服务及 IP01–IP24 在该门中仍为 `NOT_RUN`。

## 本批未覆盖

- 没有在已安装原生产品上完成代理设置页的 claim 后交互；本次浏览器证据来自一次性 managed-configuration fixture，不是 IP01–IP24 安装验收。
- 没有调用真实外部模型 Provider、读取或提交真实 API key、proxy credential 或 `.forme` 数据。
- 没有执行真实 HTTP 代理连通性、Windows/Linux/macOS 系统服务安装或完整 IP01–IP24 联合验收。

## 真实浏览器增量验收（2026-10-26）

本次使用仓库已有的 `FORME_CONFIG_BROWSER_PREVIEW=1` managed-configuration fixture，实际启动临时 `forme-hostd`、`forme-webd` 和 `forme-gatewayd`，浏览器使用临时账号登录正式内嵌 Web 工作台。模型请求由本地计数型 fixture 提供，不读取用户配置、真实密钥或 `.forme` 数据；浏览器与临时进程结束后已停止。该验收只补充 Web 代理控件的真实交互证据，不提升 IP01–IP24。

| 场景 | 实际结果 |
|---|---|
| 登录并打开设置页 | PASS；页面显示已连接正式运行时，设置页显示“网络代理：可配置”。 |
| 直连切换 HTTP | PASS；出现空的写入字段，已保存地址不进入输入值，只显示重新输入提示。 |
| 非法代理地址 | PASS；提交 `https://proxy.invalid/path` 返回 HTTP 422，配置未被接受。 |
| 合法代理地址 | PASS；提交 `http://127.0.0.1:8080` 返回 HTTP 200；响应正文、页面正文和代理控件均不包含该 endpoint 或模型凭据。 |
| 刷新与切回直连 | PASS；刷新后仍只显示 `endpoint_configured` 的脱敏状态；切回直连后设置恢复为 direct。 |
| 浏览器持久层 | PASS；代理地址没有写入 `localStorage` 或 `sessionStorage`。 |

脱敏控件截图：[proxy-control.png](evidence/web-browser-20261026/proxy-control.png)，SHA-256 `D8C709DBE417BBF83281DFC175F3C3E14343AF05E129B580301AF295307295B2`。截图没有 endpoint、凭据或用户数据。

本次真实浏览器验收仍不证明外部 Provider 可用、代理网络连通性、系统服务安装、受管原生身份、LAN/TLS 或完整产品 IP01–IP24。
