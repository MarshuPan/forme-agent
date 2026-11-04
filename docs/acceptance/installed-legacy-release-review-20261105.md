# 缺基线历史升级：独立审查组件验收（2026-11-05）

本报告承接 owner 批准的[历史审查协议](../architecture/proposals/installed-legacy-release-recovery-review.md)。范围是协议、产品 journal 与 Host 隔离实现；不授权或证明本机迁移。原升级没有恢复基线时，正常 `AbortBeforeSwitch` 仍拒绝；历史通道不补造旧基线，也不重新执行原 Upgrade。

## 1. 本轮纠正与实现

此前 `8078763` 实现了私有 journal，`80e8e7d` 将其场景加入宿主门；这些证据没有覆盖实际 Host 方法。文档曾笼统称“隔离实现完成”，本轮将状态拆成 journal、Host、HTTP/Web 与原生收集器分别记录，不回写历史回执。

- 新 Host 接缝提供报告读取、新决定、原号核实、单项及列表查询。Configuration 域认证与持久 owner 绑定先于材料检查；LocalAdministrator、AgentExecution 或请求自报 owner 不能代替它。产品口令重新认证仍属于尚未接线的正式 HTTP 认证层。
- 独立预置 Ed25519 公钥严格验证报告的 domain digest；报告不能提供验证公钥。可信内部 collector 保持材料/数据观察，Host 另外核对真实 worker 静止锁、配置/活动发布、journal CAS 和服务观察，并在提交前再次检查。
- 八类证据必须闭合、顺序正确、绑定一致；报告签名不代替实际材料来源与语义检查。生产没有挂载 collector/信任配置，默认 unavailable，不提供默认 Verified 实现。
- 原 takeover 必须已到期。首次请求及显式核实各有最多 120 秒的新窗口，均不超出报告期限；核实可晚于首次请求期限，不能修改原升级授权。提交时间不能早于 intent。
- 同 key 同 request 只读原号，过期或 intent 后也不自动推进。原号显式核实重新取得观察；单个 committed fact 原子结束原升级为 Aborted、新决定为 Committed，保留旧字节/hash、失败、marker 与原 active release。
- 当前 journal reader/updater 探测统一为 3/3。普通恢复兼容准确的 2/2 或 3/3，历史入口仅 3/3；混合、旧版和未知未来组合拒绝。能力探测使用实际离线子进程，不读取配置或模型凭据。

文档细化提交为 `791d234`，实现提交为 `8991875`，施工分支为 `shu/bootstrap-claim-journal`。本轮没有合入 main 或推送。18-crate 图、99 个脑 EventKinds 与五个既有 unsafe 边界未增加。

## 2. 验证层次

Host fixture 使用八份真实临时文件、真实临时配置/journal、实际 `WorkerExecutorHost` 的静止锁和原创测试签名。SCM 状态、历史材料语义及数据独占观察由受控 adapter 提供，因此它证明 Host 验签/绑定/重查/拒绝与 journal 原子性，不证明本机历史材料或原生 SCM 已通过。

| 层次 | 命令/范围 | 结果 |
|---|---|---|
| 协议 | `cargo test -p forme-protocol --test installed_legacy_release_recovery_contract` | 6 passed / 0 failed；闭合证据、摘要字段、封闭输入、独立核实 digest、view/list、精确版本组合。 |
| 定向 Rust | `cargo test -p forme-execution --lib legacy` | 30 passed / 0 failed；包括 Host 5 项、历史恢复 journal 7 项及既有 legacy/bootstrap 回归，不将重复执行相加。 |
| 实际离线子进程 | `cargo test -p forme-execution --test installed_release_recovery_capabilities` | 1 passed / 0 failed；3/3 输出及错误/额外输入拒绝，不打开 Host 配置。 |
| 全工作区源码门 | `pwsh -NoProfile -File tools/verify-code-quality.ps1` | PASS，exit 0；180 个 Rust targets，1168 passed / 0 failed / 15 默认 ignored；fmt/check/strict Clippy、21 项结构/资源工具回归、810 文件规模门（63 项债务未增长）、前端 100/100、bundle 10/10、原生包/POSIX 18/18、签名验证 5/5、NTFS 8/8、防火墙只读策略 25/25、依赖审计和完整合规通过。 |
| 永久宿主命名门 | `pwsh -NoProfile -File tools/verify-installed-host.ps1` | PASS，exit 0；266/266 个命名场景，每项实际恰好执行一次；含本轮 13 项增量及全部七项历史审查 journal 场景。 |
| 适用核心回归 | `tools/verify-core-brain-closure.ps1 -Wave A/B/C` 及 `-Wave D -RuntimeOnly`（分别调用） | 全部 exit 0；命名场景实际执行数分别为 47/47、50/50、36/36、71/71，冻结 18-crate/99-kind 契约与可移植工件验证通过；不替代 C24 live 或 D/Final。 |
| M5 / S1–S99 | `tools/verify-m5.ps1` | 独立在干净提交上执行，最终命令结果与 current-tree 回执统一见 [M5 报告 §13](m5-acceptance-report.md#13-历史审查组件后的当前源码重验2026-11-05)，不以旧回执或本表的组件结果代替。 |

新增永久门覆盖六个协议场景、五个实际 Host 场景和两个时间/来源反例；此前五个 journal 场景继续保留，历史审查 journal 现共七项。关键反例包括八项材料各自缺失/变化、签名错误/外来公钥、外来身份、无 collector、Brain 在运行、updater 不明/旧版本、worker/config/head 漂移、intent 中断与重启、请求替换以及 hash 自洽但语义不符的源绑定。

日志保存在本工作树 `target/legacy-release-review/`，不含真实密码、密钥或用户正文；不是远端发布工件。

统一门未提供 `-WindowsInstallerToolchain`，因此实际 Inno 编译器退出夹具为 NOT_RUN；NTFS 临时权限和只读防火墙策略也不证明真实服务 token、管理员规则或原生安装通过。15 项 Rust 默认 ignored 保留原边界，未计为通过。

## 3. 仍未闭合的产品义务

1. 真实原生材料收集器：独立来源的八类材料、原 updater 代码/构建审计、历史 worker 连续性、当前只读解密与数据独占检查，以及持锁至落盘的真实实现。缺项必须为 Unverifiable，不能改用当前静止或 owner 批准代替。
2. 受保护信任配置与独立签名管理 reader 修复：实际六程序、四服务登记/账户/恢复策略和一次性修复回执，旧 writer/updater 不得在 v3 facts 后再启动。
3. 正式 Host HTTP/Web 接线、operation-bound 口令重新认证、刷新/断线查询与浏览器验收。当前 public Host 方法不是已开放的产品路由。
4. 精确本机迁移计划及另行批准的实际程序替换、journal 接入、服务启停和失败保留。当前未执行这些动作，也未重跑原升级。
5. 完整 IP14–15/IP24、真实 Provider 和 Linux/macOS 原生验收；本组件不提升这些状态。

旧脑首次认领的发现、数据 worker intent/复制恢复、Web 账号事务与 active data/config 联合激活仍按[其独立报告](installed-bootstrap-legacy-claim-journal-20261104.md)推进，不因本历史恢复组件而视为完成。整体交付目标继续 active。
