# Web 会话操作与刷新恢复验收（2026-11-08）

本记录承接 prd/26 的会话重命名、归档、并发版本和原命令核实义务，实现提交为 `8ada1f6`。旧安装的真实对话证据与修正版临时进程证据分别记录；两者均不宣称完整 IP17/IP24 已通过。工件位于本工作树 `target/native-legacy-review-20261106/`，不包含登录密码或模型密钥。

## 1. 旧安装的真实对话

实际安装版本为 `forme-windows-x86_64-20261107-cf3fbc9`。浏览器提交的原 run 为 `run:1794142823989257300:1`，session 为 `session:sha256:10efc3c85dc2cc567748b51770b2d43b0d5a2c619d56d32c009fd595fca2dc51`。真实后端观察为 Complete、19 个事件、一次模型调用、零 ActionStarted；刷新并重新选择原会话后，原回答和事件保持，没有新增模型调用。

证据是 `browser-live-20261108/before-refresh-backend.json`、`after-refresh-backend.json`、`conversation-before-refresh.png` 与 `conversation-after-refresh.png`。此项只证明旧安装中的该次对话及历史恢复，不能证明后述修正版已部署或文件 worker 已成功。

## 2. 发现的问题与修复

旧安装点击“重命名”没有打开编辑器，控制台实际报错 `prompt() is not supported.`。前端原生 script dialog 无法在该浏览器使用。修正版用页面内表单完成重命名和归档确认，沿用正式 `updateWorkbenchSession` 与既有 `ExactOperation`，不修改 DTO、trait、crate 依赖、脑 EventKind 或治理。

表单打开时固定原 session 和版本；提交前校验非空、200 UTF-8 字节和控制字符。未知结果保留同一命令与参数，仅允许核实原操作；409 冲突必须显式重新读取。编辑期间锁定会话切换、workspace 变更与发送，Escape/取消恢复按钮焦点并保留未发送草稿。已确认的修改与后续列表刷新分开处理，刷新失败不会把已成功修改改称副作用未知。

## 3. 修正版真实浏览器组件验收

使用实际 Host/Web/Core 临时进程和独立测试账号，入口为 `http://127.0.0.1:7230`；不是前端开发服务器，也不是已安装系统服务。全部后端观察均针对同一临时 session。组件回执 `browser-live-20261108/session-actions-component-receipt.json` 记录九次独立 API 观察及对应工件摘要。

| 场景 | 实际结果 |
|---|---|
| 输入 80 个汉字（240 UTF-8 字节） | 拒绝；标题和版本保持。 |
| Escape 取消 | 表单关闭、焦点回到按钮，草稿保持。 |
| 有效重命名 | 正式后端提交新标题和新版本。 |
| 保存相同名称 | 没有新增版本。 |
| 表单打开后由另一正式 API 修改标题，再提交旧版本 | 409；并发标题和版本保持，无自动覆盖或重试。 |
| 显式重读后重新编辑 | 新版本的重命名成功。 |
| 取消归档 | 后端记录和版本保持。 |
| 确认归档 | 列表移除该会话；后端保留 archived 历史记录。 |
| 刷新页面 | 归档结果保持。 |

九次后端观察均为零 run、零模型调用，草稿从未提交。截图 `session-actions-conflict-confirmed.png`、`session-actions-archive-confirmation.png`、`session-actions-archived.png` 与 `session-actions-rerun-invalid-name-viewport.png` 对应上述真实 UI。临时实例已经停止，自己的临时目录已移除，7230 listener 已关闭。

首次夹具因证据文件名校验而退出，原件保留：`session-actions-invalid-name.png/.txt` 实际是过期夹具登录页，不能作为输入校验证据；`session-actions-conflict.png` 是冲突提交前的渲染帧，应使用 `session-actions-conflict-confirmed.png`。本轮没有在浏览器注入丢响应故障，该分支仅有既有 ExactOperation 回归证据。

## 4. 源码与安装验收边界

完整源码质量门已 exit 0，终线为 `Code quality: PASS`，日志为 `session-actions-quality-retry-20261108.log`。

| 检查 | 实际结果 |
|---|---|
| 完整 Rust workspace | 183 个测试目标，1304 passed / 0 failed / 16 默认 ignored。 |
| Rust fmt、全部 targets check/build、strict Clippy | PASS。 |
| 结构/资源/字节一致性 Python 回归 | 21 tests，OK。 |
| 代码规模棘轮 | 881 files / 63 existing size debts，未刷新基线。 |
| 前端 TypeScript、行为测试、嵌入资源可重复生成 | 115 passed / 0 failed。 |
| 前端依赖审计 | 0 vulnerabilities。 |
| bundle、原生包/PE/POSIX、真实 Rust 签名验证组件 | 分别 10/10、20/20、5/5。 |
| NTFS 与防火墙只读策略组件 | 分别 10/10、25/25；不代表管理员防火墙生效验收。 |
| 独立实际 Inno 退出夹具 | 2/2，日志 `session-actions-inno-20261108.log`；不安装 ForMe、不提权、不更改服务。 |
| 原创性、许可证、复制检查与 diff whitespace | PASS。 |

首次调用在链接 gateway binary 时因临时浏览器进程占用程序文件而 Permission denied，原失败日志 `session-actions-quality-20261108.log` 保留；停止该夹具后沿相同实现运行的第二次完整门成功。统一门未传 Inno toolchain，因此其中该分支如实输出 NOT_RUN；实际 Inno 结果来自上表的独立执行，不改写原日志。本轮没有重新运行完整 `verify-m5.ps1`，独立发布树审计也不能代替该脚本的完整终态。

修正版尚未部署，旧签名候选 `forme-windows-x86_64-20261108-3b9a1cf` 不包含此次 UI 修复，不得改名复用。当前 Host/Web/Brain Running、updater Stopped 是旧安装观察。最终干净 commit 必须另取当前树审计和新打包回执，实际升级后继续文件矩阵与完整 IP 联合验收；本报告不提升任何整体验收状态。
