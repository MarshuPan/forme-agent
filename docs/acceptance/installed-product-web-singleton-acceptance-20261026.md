# Web 单例争用与本地浏览器验收

日期：2026-10-26

## 范围

本记录补充 installed-product 的 Web 进程边界证据。它验证普通认证库的单写者/单 Web daemon 契约、重启后的认证与配置恢复，以及真实本地 Chrome 对登录入口的渲染。它不替代原生安装、系统服务、跨平台权限、真实外部模型或 IP01–IP24 联合验收。

## 自动化证据

命令：

```text
node --test tools/tests/web_multi_process.test.mjs
```

结果：PASS。

覆盖内容：

- disposable Host 启动后，真实 `forme-webd` 建立认证存储并完成一次新脑认领；
- 第二个 Web 进程使用同一认证库尝试启动时，在 singleton writer 边界失败关闭；
- 第一个 Web 进程在争用失败后仍可读取 owner session 并完成一次配置提交；
- 停止并重新启动第一个 Web 进程后，owner session、配置 revision 和模型配置仍可读取；
- fixture 只使用临时安装目录和仓库内测试凭据，不读取 `.forme`、真实 owner 数据或真实密钥。

这对应 `prd/24` 的“普通认证仍只有一个 Web daemon”约束。第二个 Web 进程被拒绝是预期结果，不是跨进程共享认证写者的能力。

## 真实浏览器证据

命令：

```text
$env:FORME_BROWSER_PREVIEW='1'
cargo test --locked -p forme-gateway --test web_browser_preview -- --ignored --nocapture
```

真实 Chrome 以独立临时 profile 打开预认领的本地 Web 地址并生成登录页截图。截图只证明静态入口在真实浏览器中可渲染，预览核心保持离线，未执行真实模型或工具动作。

证据文件：[singleton-login.png](evidence/web-browser-20261026/singleton-login.png)

SHA-256：`sha256:b8a034f7b35114b2e694bf98d5c872ea998a02c03338cb7a307309640ca2ada2`

预览过程结束后，fixture 和浏览器临时 profile 均已清理。

## 未提升的结论

- 不宣称正式安装包、SCM/LocalSystem、Linux systemd、macOS LaunchDaemon 或签名发布已通过；
- 不宣称真实 provider、真实工具执行、附件恶意内容隔离或完整 IP17–IP24 已通过；
- 不宣称整个 installed product 已完成；该记录只闭合本地 Web 单例/恢复和浏览器渲染的组件证据。
