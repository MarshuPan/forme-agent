# Web 窄屏、键盘与跨标签页退出组件验收

2026-10-26，基于 `shu/installed-product` 的实现提交 `5391750e6f968e5690692636a8797c14206eba53`，通过本地真实浏览器检查内嵌工作台。结论限于本文列出的组件；IP01–IP24 没有因此变为 PASS。

## 1. 环境与复现

主机为 Windows `10.0.26200.0`，当前进程非管理员；本机没有注册的 `forme-*` 服务。使用 ChatGPT in-app browser；未取得可核实的浏览器版本，因此不填写版本号。

手动夹具为 `crates/gateway/tests/web_browser_preview.rs`。它在独立临时目录中通过正式 identity store 建立公开测试账号，启动真正的 `forme-webd`，但不配置 Host、core、实际 owner 或模型。不是 Vite 页面，也不是完整首次认领测试。夹具默认 ignored，只有显式 opt-in 才运行：

```powershell
$env:FORME_BROWSER_PREVIEW = '1'
cargo test --locked -p forme-gateway --test web_browser_preview claimed_web_daemon_for_browser_component_checks -- --ignored --exact --nocapture
```

跟随 stdout 给出的本次 loopback 地址操作。完成后在 stdout 指定的临时 `PREVIEW_STOP` 路径创建停止标记，由夹具 kill/wait 子进程并清理临时目录；不得使用实际账号、数据目录或密钥。最后一次运行 1/1 PASS，停止后临时目录不存在。测试账号凭据只在夹具源码中，不在报告里复制。

## 2. 浏览器观察与修复

| 检查 | 实际结果 |
|---|---|
| 390×844 窄屏导航 | 展开后焦点落在“对话”，Tab 到“工作空间”；Escape 关闭并将焦点交还“导航”。选择页面也关闭导航并恢复焦点。 |
| 320×740 最小视口 | 顶栏按钮保持横向文字，服务状态单独一行；设置页标题正常换行，无横向溢出。 |
| 宽度断言 | 390 视口的 clientWidth/scrollWidth 均为 375；320 视口均为 305。系统纵向滚动条占用 15px，未通过隐藏横向滚动掩盖越界。 |
| core 离线维护页 | 登录仍可用；页面明确显示主脑和配置服务未配置，诊断与脑数据操作禁用，不伪造连接或另建主脑。 |
| 两个标签页退出 | 同 origin 共享登录，一页退出后另一页立即返回登录；原 app-shell 消失，输入清空。刷新后仍未登录。 |

首次检查发现 `body` 的 320px 最小宽度与系统滚动条共同导致横向溢出，顶栏按钮和服务状态也可能被挤成竖排。实现去除该最小宽度、固定按钮不收缩，并为最窄视口采用两行顶栏。另补导航展开/关闭的焦点管理、Escape 与认证失效时的展开状态重置。未修改后端协议或权限语义。

导航截图曾捕获到登录处理中，复核时已舍弃并补拍；下列入库文件为稳定的最终画面。尺寸是截图 API 返回的内容区尺寸，不能当成测试视口尺寸。

## 3. 证据清单

| 文件 | JPEG 尺寸 | bytes | SHA-256 |
|---|---|---|---|
| [导航展开](evidence/web-browser-20261026/navigation-390.jpg) | 375×812 | 16594 | `aa94c4e0cb96d1124821bc1b811c7c5f4af9c31f7041b27fd3a53c7debca5497` |
| [窄屏离线维护](evidence/web-browser-20261026/offline-settings-320.jpg) | 305×660 | 15618 | `1097d568777349ab0500898b66ab49b68c17862912981894c847a63828fd9b55` |
| [另一标签页退出后的登录页](evidence/web-browser-20261026/peer-logout.jpg) | 1280×720 | 32099 | `b84e49e90de09a10671be9878b0c459fd08e46c3f618550b03afd6a683617c96` |

内嵌构建输入 source_digest 为 `8114f8debda8aa11e253675b339102811924bc71687577aa6b28605a81cffedf`：

| 静态文件 | bytes | SHA-256 |
|---|---|---|
| `assets/index-BbhQ1fO8.css` | 14769 | `4375a8d4833de33a7f9616a54330385a391954c6c0b0366de498b8f47a313aa7` |
| `assets/index-BZSLlqWV.js` | 368274 | `cb3aa3860b5c3508efb8ef12441fde302eb0fc1a129ffaf66f6015ad2bed17ab` |

## 4. 源码验证与边界

`npm run verify --prefix frontend` 的类型检查、81/81 行为测试和精确静态构建核对通过；`cargo fmt`、workspace all-targets check、strict Clippy、代码质量门和原创合规门通过。规模棘轮为 724 文件、63 处既有债务，无增长。`verify-web-workbench.ps1` 的 8 个命名组件场景通过。新增浏览器夹具是手动辅助测试，不把默认 ignored 计入通过数。完整 M5 与发布树审计结果另行记录，不能从这组组件证据推导其通过。

未覆盖首次安装/认领、改密码、真实 provider、脑数据及工具副作用、Web 多进程争用、真实服务重启、LAN/TLS 或原生包联合回归。本次两标签页只验证认证退出同步，不能替代对话、审批、认知纠正等操作的并发恢复验收；完整 IP17/IP18/IP20/IP22/IP24 仍开放。
