# 首次安装权限分区与服务访问组件验收

日期：2026-10-22。分支：`shu/installed-product`。设计说明 `76803c8`，受验实现 `29984a4`。本记录对应 prd/24 §4.4、prd/25 §11.6 和工程质量规范；不是原生安装包、平台强隔离或 IP01–IP24 完成声明。

## 1. 实现范围

新安装的 core 发布配置为 `config/core/core.json`；会话认证 fence 和加载确认移到 `brain/runtime/core-auth.sqlite`、`brain/runtime/core-config-ack.json`。主脑不再需要写宿主管理或配置目录。`config/core` 与 `keys/model-credentials` 预建，后续原子替换/不可变 revision 继承各自读取授权。Brain runtime 在认领后按需创建，安装准备仍保留空的新脑目录。

重复 `prepare-installation` 读取并校验原 installation 与有界 template。新旧布局都按原模板报告已有 core 文件，不改写模板、密钥、ack/fence 或数据。模板缺失、版本/安装不符、路径逃逸/混搭拒绝；旧布局不自动升级为新的原生服务安装。

Windows 新增原创有限入口 `install-access.ps1` 和 `service-access.psm1`，仅随安装器调用，使用 Windows PowerShell/.NET 公开 ACL API；没有通用 shell 路由、新 registry 包或内部依赖边。角色矩阵的目标为：

| 资源 | SYSTEM/Administrators | Web SID | Brain SID |
|---|---|---|---|
| 程序版本 | 管理 | 读/执行 | 读/执行 |
| `host-state`、`backups`、管理凭据 | 管理 | 无授予 | 无授予 |
| `web-auth` | 管理 | 修改 | 无授予 |
| `brain`、`workspaces` | 管理 | 无授予 | 修改 |
| core 发布配置、模型凭据 revision | 管理 | 无授予 | 只读 |
| `core-owner.token`、`agent-execution.token` | 管理 | 无授予 | 只读 |
| `core-web.token` | 管理 | 只读 | 只读 |

共享父目录只授予非继承的遍历/属性查询；日志分开，worker 不从本矩阵获得权限。这里的“无授予”由 descriptor 检查证明，尚未以真实 SCM 服务 token 做有效访问拒绝验收。

安装器先保护空默认根，再写 payload/secret；顺序 prepare、注册三个服务、设置/回读 ACL，最后启动 Host/Web。每步检查退出状态，Brain 等认领和模型配置。已有安装/非空目标拒绝接管，失败保留受保护现场。卸载先确认保留范围，逐个调用 typed stop/unregister，失败阻止后续程序移除。这仍不等于带 product journal 的完整升级/恢复/卸载事务。

Linux/macOS 的 Brain 启动模板同步引用新发布路径；本批没有完成它们的账号/文件权限与原生包演练，不以路径一致性测试声明平台可用。

## 2. 实际执行与结果

环境为原生 Windows，当前进程不是管理员，也没有安装 Inno Setup 编译器。权限测试只在本轮独立临时目录中创建自建 fixture，不注册服务、读取真实账号/模型配置或操作 `.forme`。

| 检查 | 结果与证据边界 |
|---|---|
| `pwsh -File tools/verify-code-quality.ps1`（默认完整合规） | PASS。结构回归 6/6，438 个源文件、63 处历史规模债务未增长；fmt、workspace check/test、strict Clippy、npm audit、TypeScript/前端、资源、bundle、Windows NTFS 组件和合规均通过。 |
| 全量 Rust（上门内执行） | 146 个 suite 汇总为 752 passed / 0 failed / 5 默认 ignored；不累计后续专项复跑。 |
| `cargo test --locked -p forme-cli --test hostctl_install` | 6/6；含新路径分区、空脑、重复 prepare、已发布配置、旧布局保留和错误模板零新写入。 |
| `powershell.exe -NoProfile -NonInteractive -File tools/tests/windows_service_access.tests.ps1` | Windows PowerShell 5.1 原生执行 7/7；不是 mock 文件系统。 |
| 同一脚本由 `pwsh` 执行 | PowerShell 7.6.6 原生执行 7/7，验证公开 API 的两种运行时适配。 |
| 前端/bundle | 14/14 前端行为与 9/9 bundle；70 个锁定 npm 依赖、漏洞审计为零，7 个嵌入资源可重复校验。 |
| `tools/verify-installed-host.ps1` | 70 个命名测试 PASS；生产隔离缺失仍 blocked，服务注册只取 adapter 契约证据。 |
| identity / ingress / workbench 门 | 分别 18/18、8/8、8/8 PASS；包括实际 Web 进程 smoke、同一 ProductionRuntime 与会话 fence。 |
| `tools/verify-product-configuration.ps1` | 16 个 Rust 配置/激活场景及 1 个实际 host/web/core 进程场景 PASS；使用本地计数模型，未调用真实 provider。 |
| `tools/verify-core-brain-workspace-contract.ps1` | PASS，冻结 18-crate 依赖图、99 EventKinds 与历史 strict prefix 不变。 |

NTFS 七项覆盖：固定角色/凭据矩阵；父级遍历不扩散；原子创建时带 DACL、精确回读与篡改拒绝；发布替换及嵌套 revision 继承；非规范路径、junction/hard link 拒绝且目标保留；有限入口对非默认根零写入；实际 hostctl 输出与权限布局联合校验、旧/异安装/超大输入拒绝及密钥/空脑保留。测试使用自建 SID，当前用户保留 fixture 清理权，不冒充真实服务登录。

中间回归曾出现两次 loopback 传输失败：已有 worker HTTP 测试报 Windows 10053，以及并行全量检查期间一次 bootstrap-proof 返回 Unavailable。没有通过增加生产重试、吞掉错误或放松断言消除失败；单项检查、最终完整源码门及最后串行产品组件门均通过。连接中止的根因尚未确定，保留为本机测试环境的待观察项，不计为成功案例。

以下本机日志仅用于核对本批执行，不是签名发布回执；不随文档提交原始 fixture 输出：

| 日志名 | SHA-256 |
|---|---|
| `forme-install-access-quality-final-86a9ffd57272427da97d43c7e5cb9beb.log` | `6869F5419662133B8E3B1A7131E234C5B8142C84B6BC54D4AF17868A9755BACB` |
| `forme-install-access-components-final-b061ce84c2bf40e89e8bf0885abc3a11.log` | `3E255FA17855A05C76D1C0882C9A973553C86D631C91CD845D580876D4F3F41D` |

## 3. 仍未验收或实施

- Inno Setup 编译、签名与真实管理员安装/卸载；没有生成可发布安装包。
- SCM 真实虚拟账户的登录、文件有效权限/拒绝、开机无人登录运行；实际 ACL helper 的管理员整机运行未执行。
- 逐 job 文件/网络强隔离、会话 helper、core 到隔离 worker 的正式工具接入；普通进程不能冒充安全 sandbox。
- 完整安装/升级/回滚/恢复事务、旧布局显式迁移、历史脑/备份接管、LAN TLS 和余下工作台功能。
- 新的完整 M5/golden/release audit、真实 provider、浏览器联合安装流程以及各平台 IP01–IP24。

本批代码仍在产品化分支，未合入 main、未推送。当前整体状态继续见 [实施记录](installed-product-implementation-progress.md)。
