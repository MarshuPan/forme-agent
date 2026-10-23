# 运行诊断、对话恢复与本机恢复权限组件验收

验证日期：2026-10-24。施工分支 `shu/installed-product`，实现提交 `72db326`；文档/依赖审查为 `fded03f`。本报告只记录本组件证据，不关闭独立部署/Web 产品里程碑或 IP01–IP24。

## 1. 实现与安全边界

- 设置页通过已认证、精确 GET 的 `/api/v1/brain/runtime/doctor` 读取正式 core 的只读诊断。POST、query、tick/reload 等路径继续拒绝；重复查询不推进五个脑聚合水位，不调用模型。前端校验版本、有限字段和阻断一致性；离线或请求失败清除旧结果，晚到响应不能覆盖新状态。装配 ready 不表示真实模型、工具或消息渠道已连通。
- 对话从原 run 的 `RunAccepted(UserTurn)` 恢复用户原文，没有第二份聊天数据库。来源、run/session 绑定和事件连续性必须正确；后台任务与子代理输入不显示为用户发言。终态同步侧栏，较旧列表响应不能覆盖较新运行水位；选择历史会话打开该会话最新 run。归档会话的发送/上传在前端禁用，服务端原有归档约束不变。
- `recover-login` 在读取配置、标准输入或认证库前检查真实 OS 权限：Windows 使用进程 token，Unix 使用有效 UID。取消 PATH 中 `whoami/id` 与本地化文本解析，避免伪造程序或 deny-only 管理员组误授恢复权限。本机测试实际非 elevated；没有重置真实账号。
- Tokio 从 `1.52.3` 精确升级到 `1.53.3`，保持 features、18-crate 内部依赖图与 99 EventKinds 不变；NOTICE/依赖审查同步。依据本机官方 RustSec 库 `RUSTSEC-2026-0386` 的 patched 范围处理，没有忽略 advisory。
- 配置进程 fixture 先证明未登记 session 被生产校验拒绝且模型调用数不增长，再通过正式 session 创建 API 发 run；未放宽生产校验。浏览器 fixture 复制自有二进制到独立临时目录，避免占用 Cargo 输出。

## 2. 源码与进程验证

| 门 | 结果与证据 |
|---|---|
| fmt、locked workspace check/test、strict Clippy、完整 compliance、code-quality、diff check | PASS；158 suites，852 passed / 0 failed / 5 默认 ignored；572 个受检文件、63 处既有规模债务。日志 `target/installed-product-full-20261024-web-diagnostics.log`。 |
| 前端 strict TypeScript、行为验证与静态资源可重复检查 | PASS；49/49。`npm audit --prefix frontend --audit-level=moderate` 报告 0 vulnerabilities。 |
| 打包/bundle/POSIX 打包与结构检查器回归 | 分别 23/23、6/6 PASS；不表示原生包安装通过。 |
| Web 工作台命名集成门 | 8/8 PASS；日志 `target/installed-product-web-diagnostics-rust-20261024-named.log`。此前错误过滤产生的 0 tests 日志保留，不算证据。 |
| 受管配置综合门 | 22 个 Rust cases 与 2 个 Node 真实进程场景 PASS；含 host/web/core、配置发布、正式 run、备份恢复与本机权限否定场景。日志 `target/installed-product-configuration-20261024-web-diagnostics.log`。 |

此前 `92a8820` 的完整 M5 执行到 S99 被 Tokio advisory 阻断，原日志 `target/installed-product-m5-20261024-blocking-io.log` 保留。更新后的独立 `cargo audit --json` 不再包含该 Tokio 发现，但存在 registry 查询超时及其他已知发现，不能单凭它宣布供应链通过。完整 M5/current-tree audit 必须重新运行，以最终退出结果另行记录，不改写历史回执。

## 3. 真实浏览器行为验证

使用仓库自建临时 installation、fixture 账号与本地计数模型，实际启动正式 host/web/core 二进制及 ProductionRuntime；未使用真实 owner、模型密钥或 SCM 服务。

1. 发送含 `<script>fixture-only</script>` 的普通文本，经 19 个正式事件到 `RunComplete`；用户原文与回答均以文本显示，侧栏同步“运行已完成”。
2. 页面 reload 后重新选择原会话，原文与回答可恢复；模型调用仍为 1。
3. 设置页诊断显示 ready/disabled/unsupported 及原因，重复读取未增加模型调用。
4. 停止 core 后刷新诊断，原正常结果消失并显示服务不可用；重新检查连接后显示“已登录 · 主脑离线”，账号/安装维护仍可访问，个人数据操作被禁用。
5. 启动原 core 后重连，原会话与 19 个事件保持，诊断重新加载；计数模型仍只调用 1 次。

页面证据位于 `target/installed-browser-evidence-20261024/`：`browser-conversation-fixed.png`、`browser-runtime-doctor.png`、`browser-runtime-offline.png`。截图与日志是本机证据，不随本报告宣称已发布。

归档确认框场景被浏览器控制接口中断，未取得完成后的页面证据，因此不记浏览器 PASS。其余上述行为已取得页面状态；自建 QA 进程及临时安装由 fixture 的 `finish` 收尾。没有停止用户 Vite、删除个人数据或注册系统服务。

## 4. 保持开放

本机管理员正向恢复、Unix 原生权限、三平台安装/服务账户、逐 job 文件/网络强隔离、完整升级回滚、旧脑/备份初始化接管、LAN TLS 与剩余设置、真实 provider 和完整 IP01–IP24 仍按实施记录开放。组件测试、浏览器 fixture 和装配诊断不替代这些证据。
