# 当前安装与 Web 工作台只读联合检查（2026-11-13）

本记录承接受管消费者、桌面授权、截图结果和渠道组件闭合记录，范围是当前本机已安装版本的只读服务、认证 API 与 Web 工作台检查。它不把已安装旧版本的健康状态提升为当前分支部署成功，也不关闭 IP01–IP24。

## 检查范围

- 使用当前本机已注册的 Windows SCM 服务和认证测试账号读取安装状态；没有启动、停止、升级、回滚、卸载服务，也没有写入产品配置、脑数据、密钥或渠道消息。
- 通过 Ego Lite 登录 `http://127.0.0.1:18765/`，打开“设置与维护”页面并保存截图。浏览器任务空间在检查结束后已关闭。
- 当前源码工作树为 `shu/product-integration@b1d7305`；现场运行版本仍是 `forme-windows-x86_64-20261109-36f8662-dev`，因此本检查不证明最新提交已部署。

## 实时结果

服务身份与状态（2026-11-13 12:06，非管理员只读进程）：

| 服务 | 状态 | 服务身份 |
|---|---|---|
| `forme-host` | Running | `LocalSystem` |
| `forme-brain` | Running | `NT SERVICE\\forme-brain` |
| `forme-web` | Running | `NT SERVICE\\forme-web` |
| `forme-release-updater` | Stopped | `LocalSystem` |

认证后的四个产品接口均返回 HTTP 200：`/api/v1/product/status`、`/api/v1/product/maintenance`、`/api/v1/product/config` 和 `/api/v1/brain/runtime/manifest`。产品状态为 `ready`，核心状态为 `ready`，配置版本与确认版本均为 3。

运行时清单显示：

- 核心认知、编排、模型和文件后端为 `ready`。
- `notification-backend` 为 `blocking`，原因是 `managed worker binding is missing or unavailable`。
- shell、browser、computer、PTY 后端为显式 `disabled`。
- MCP、App API 和 plugin registry 为 `unsupported`，原因是当前安装没有 owner-configured consumer。

认证 Web 工作台显示“主脑已连接”，设置页显示当前版本、服务状态、运行诊断和上述阻断项；截图保存为 `target/product-integration-evidence/installed-settings-20261113.png`。

## 证据文件

- `target/product-integration-evidence/remaining-installed-read-20261113.json`，SHA-256 `7e9a266873c784fcb982f0d22ae3edcd716d6f0f2aa2f808e01ad2e46aa64e18`。
- `target/product-integration-evidence/remaining-current-services-20261113.json`，SHA-256 `8647de16950e6375e42357a027e222d9a437e6b1608998d08c2b98069ff07f7d`。
- `target/product-integration-evidence/installed-settings-20261113.png`，SHA-256 `95dca44ca2ea13c1519f11b0602973fb1bc235a2f1a92293d6bfe507250bcbfb`。

## 结论与后续边界

本机已安装底板、SCM 身份和认证 Web 读路径均可观察；产品仍未具备可用于联合验收的 managed worker binding，当前版本也不是本分支最新构建。没有取得当前提交的签名安装候选、管理员/UAC 安装证据、真实 AppContainer/桌面 helper、真实 MCP/插件/渠道消费者或外部渠道投递证据，因此 IP01–IP24、Final 和安装联合验收继续保持未完成。未知出站结果仍必须保留 Unknown，不重试或改写为 Delivered。
