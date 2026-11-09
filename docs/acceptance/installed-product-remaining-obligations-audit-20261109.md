# 安装产品剩余实施与原生验收义务审计（2026-11-09）

本清单的原始审计以 `shu/closure-inventory@6899f56` 和当时真实安装状态区分实现、源码回归、安装验收。本轮完整源码 Final 已取得 exit 0，详见[核心终验记录](core-brain-closure-acceptance-report.md#7-纠正-scope-与闭合清单后的完整源码终验2026-11-09)。本审计不是安装或发布声明，不把局部成功提升成整个场景 PASS。原始范围保持为 requirements/17 P01–P16、requirements/18 IP01–IP24、PRD23 C1–C24 与历史 S1–S99，以及工程维护规则。

后续实施状态：owner 已明确批准受管 MCP/插件/渠道、桌面授权与截图输出三项协议并要求实施，同范围不再等待确认。安装恢复和签名升级已推进到 `36f8662-dev`，详见[现场启动修复](installed-startup-reconciliation-20261109.md)；受管配置和桌面授权分别见[设置组件](installed-managed-settings-20261109.md)与[授权组件](installed-desktop-authority-20261109.md)。下列原始窗口与 Final 回执保留其历史源码绑定，不代表这些后续实现或整组 IP 已通过。

## 1. 已有证据及正在执行的原号

| 层级 | 已取得的证据 | 当前边界 |
| --- | --- | --- |
| 当前整仓源码 | `6899f56` 的完整 Final 实际 exit 0；183 个 Rust test targets：1,310 passed / 0 failed / 16 默认 ignored，strict Clippy、完整 compliance、Python 94 passed / 1 skipped。 | 默认 ignored 不计通过；两次 workspace 阶段不叠加计数。该分支未合入 main。 |
| 当前四波 | Final 已逐名复验 A/B/C/D 47/50/36/73，共 206 项，并通过便携工件与 typed verifier。 | 两个新增 C23 场景在 D 清单内；C24 真实证据保留原采集和源码绑定，不宣称本轮再次调用真实模型。 |
| C24 真实模型 | 六类工作流、各两轮，primary 与 baseline 均有真实 profile 和评分证据；unknown cost 保留。 | 已验证该组任务，不证明长期收益或安装版 IP，更不声称 forme 质量胜过直接模型。 |
| 当前审计回执 | core `7bcc6fac780a1c1669f8dd527e4e283cdbbfc11ea140ee7086fb8c676c047069`；M5 `78eb9f2e29a19ddaac0abe638e082bdd1e871953b77b646a18a5acac4dabfb80`。 | Final 内重新生成、独立 verify 与当前干净树 compare 均 PASS；这些是本次 `6899f56` 的回执。文档记录提交后，规范回执须按现有规则重新绑定。 |
| 原生安装 | 窗口前版本为 `forme-windows-x86_64-20261107-cf3fbc9`；预检时 Host/Web/Brain Running、updater Stopped。10:34 后只读 SCM 为 Host/Web Running、Brain/updater Stopped。 | 正式升级原操作尚未结算，active release 与恢复须查原号；服务状态不是新版升级完成证据。当前 Product URL 为本机 loopback。 |
| 新原生候选 | `forme-windows-x86_64-20261109-7b13529` 原请求已在 10:29:58 取得管理员启动回执，候选入库 committed。 | 10:34:47 原窗口在正式升级未结算时停止，文件矩阵和数据阶段未运行；三份原件已取得，见 §7，不重新派发。 |

Final 为原 `target/closure-inventory-evidence/run-current-final.ps1` 会话，实际在本地时间 03:14:03–04:11:07 结束；exit 0、`core-brain-closure (Final): PASS`、clean tree、发布回执 compare 与末次空间报告均成立。`current-final.log` SHA-256 为 `4ab96146e99c5acafe843ea7b1d7a01323c1d74d7a3589812013aef3c4ceb080`。后续验收说明和回执提交不冒充在新 commit 上重新运行完整 Final。

原生批次保留于 `bootstrap-claim-journal` 工作树，源码固定 `7b13529`；目标为一次签名升级、文件 deny/write/read/delete、备份恢复负例与受控 StopCore/StartCore。三份启动/原生/产品回执已取得，但结果是 stopped，不能据此判验收通过。原 Unknown command 只查原件，不重新派发；最新原件和后续检查边界见 §7。

## 2. 尚需实现的安装版能力

| 义务 | 当前缺口及下一项可执行交付 | 决策状态 |
| --- | --- | --- |
| 既有能力的受管配置及消费者 | settings v3、Host 注册来源/程序/包公钥，MCP 隔离 discovery/call、插件贡献、已有渠道 adapter 与 typed M5 Web 控制。`320d0b0` / `a372499` 已修复普通 stdio 的有界 I/O 与原 cleanup 监督，详见[MCP 组件验收](installed-mcp-stdio-io-acceptance-20261109.md)；不替代受管接入或隔离。 | 已批准并实施。settings v3、公钥装配与有限 Web 路由已完成组件验证；隔离 discovery/call、插件贡献和渠道实际消费者继续实施。 |
| 桌面授权及主脑接线 | Host OS 会话目录、pending/consent/issue/revoke journal，exact grant 消费、正式 planner/backend 与 Web 会话选择。 | 已批准并实施。`7e4b7ce` 已接通 Host 授权、主脑等待/恢复、Web 会话选择；独立桌面设置和原生联合验收继续推进。 |
| 截图、引用输入和真实 PTY | helper 当前拒绝截图和 ContentRef/SecretRef Type，只处理 Computer；desktop 结果 source=None。`1207740` 证明三个隔离机制，`451f3b4` 修复普通 I/O；后续 `e9da708` 将同一真实 backend 放入隔离 fixture，八项 I/O 组合和三个原场景共 11 个独立实验全部通过。初始化/OS 关闭异常矩阵与生产接线仍未完成。 | 证据见[PTY 机制验收](installed-isolated-pty-mechanism-acceptance-20261109.md)、[I/O 组件验收](installed-pty-io-lifecycle-acceptance-20261109.md)和[组合验收](installed-isolated-pty-io-composition-acceptance-20261109.md)。仍须闭合受保护输入/结果、Host grant、生产 placement 和真实安装证据；组合机制不授权新增冻结协议，也不关闭完整 IP。 |
| 其他安装版执行 placement | Shell、Browser、AppApi、Notification 逐项核对实际消费者、隔离 profile、凭据/输入/结果与取消。 | 有通用 broker/factory 分支不等于实际安装能力；不得统一启用 flag 或回退 brain 进程内 driver。 |

追加组件证据：`1b6884f` / `d43e27a` 已闭合普通 MCP 丢响应/无效响应/副作用后取消的 Unknown，以及 Harness 的原号挂起、真实 SQLite 恢复、同键重放和持久裁决。workspace 为 188 targets、1,346 passed / 0 failed / 22 默认 ignored，required checks 与原件核验通过；详见[MCP Unknown 组件验收](installed-mcp-unknown-outcome-acceptance-20261109.md)。这不启用受管消费者，也不替代安装证据。

随后 `b131832` 闭合普通 Shell 继承管道的有界清理、八槽准入和唯一 Unknown；四类真实故障与正常路径通过。最终 workspace 为 189 targets、1,355 passed / 0 failed / 22 默认 ignored，fmt/check/test、strict Clippy、质量、完整合规及独立原件核验通过；原资源/Clippy 拒绝均保留，详见[Shell I/O 组件验收](installed-shell-io-lifecycle-acceptance-20261109.md)。安装版 Shell 的实际 placement 仍属上表义务，本次未部署或刷新完整 Final/IP。

`3803069` 另修复升级专用服务观察：原状态传播不再立即失败，相反 pending/状态回退及超期终态仍拒绝，保留单次控制和原授权期限。原三项失败与最终八项/共享三项均留证；完整 workspace 189 targets、1,363 passed / 0 failed / 22 默认 ignored，七阶段与独立原件核验 PASS，见[升级服务观察验收](installed-release-service-observation-acceptance-20261109.md)。修复尚未部署，未取得原升级受保护诊断，不据组件复现断言现场根因或 IP 通过。

本表列实施义务，不把代码中正确显示 unavailable 视为完成；也不把历史 M0 的延后能力误用来阻止已经激活的 M2/V1 设计。需要冻结变更时遵守 AGENTS.md 硬规则 3；常规实现与修复不再另造审批要求。

## 3. IP01–IP24 的逐项完成审计

以下状态针对当前整体原生场景；`UNVERIFIABLE` 包括已有部分证据但不足以证明整项，不能按通过计算。每行的最终证据仍须包含 requirements/18 规定的包/UI、OS/浏览器、基线、实际动作、独立探测及反向断言。

| 场景 | 当前判定 | 要关闭该项仍需取得的证据 |
| --- | --- | --- |
| IP01 整体安装 | UNVERIFIABLE | 当前候选在无开发工具目标的原生安装、签名/资源/服务身份、5 秒 Product URL 与错误安装负例。旧安装不能替换当前包。 |
| IP02 无登录常驻 | NOT_RUN | 原生冷启动、无桌面登录、关闭浏览器/Web 登出/OS 注销后真实后台事件；故障不依赖登录恢复。 |
| IP03 崩溃恢复 | UNVERIFIABLE | 当前安装的 OS 恢复上限、唯一 supervisor、双启动/唯一 writer、原 Unknown 不重做；组件恢复测试不足。 |
| IP04 未配置可达 | UNVERIFIABLE | 当前候选清洁安装的无模型初始化、5 秒可达及配置损坏/core 离线下可认证维护。 |
| IP05 抢占防护 | UNVERIFIABLE | 当前安装认领/中断恢复、headless 与 LAN 抢占/并发/重放/过期负例；真实认领成功只是部分路径。 |
| IP06 登录恢复 | UNVERIFIABLE | 唯一 Owner/SuperAdmin、过期/退出/撤销/改密及本地管理员恢复，独立核对 OwnerId 和脑 keys 不变。 |
| IP07 入口身份 | UNVERIFIABLE | 当前安装真实 owner/session 到 core 的审计；payload/forward header/内部端点伪造的零 model/driver 负例。已有 smoke 不等于全部场景。 |
| IP08 配置生效 | UNVERIFIABLE | 新版完整设置与真实消费者、Host 自动生命周期和 core ack/restart；失败保存、Unknown 不重试与秘密不回显。 |
| IP09 LAN/TLS | NOT_RUN | 明确启用 LAN 后的真实跨设备 HTTPS 登录、证书信任、Host/Origin/CSRF/firewall 与错误证书/HTTP 负例。 |
| IP10 权限分离 | UNVERIFIABLE | 当前候选真实 SCM 身份与 ACL、隔离 worker 正向终态及审批/permit/replay 零额外副作用。原生窗口在正式升级阶段停止，新四项文件矩阵尚未运行。 |
| IP11 桌面会话 | UNVERIFIABLE | 完成 Host 授权与正式主脑接线后，真实 session/helper/peer/action/receipt；锁屏/注销/换会话/撤权/失联分别取证。 |
| IP12 数据密钥 | UNVERIFIABLE | 当前安装真实加密/ACL、无交互重启、备份恢复和口令变化不丢解密能力；新备份/离线窗口尚待执行。 |
| IP13 旧脑接管 | UNVERIFIABLE | 真实安装的三类候选、独占锁/keys、catalog→claim→activation→runtime ack 全链及错误来源负例；隔离协议测试不足。 |
| IP14 升级事务 | UNVERIFIABLE | 原号签名升级完成排空/备份/切换/健康、原身份事件保持，以及失败阶段的可恢复证据。已有启动与停止原件，正式升级原操作未结算，不能当作完成。 |
| IP15 回滚恢复 | UNVERIFIABLE | 当前安装的兼容回滚、新路径恢复/显式 active 切换、错误 schema/key/plan 与原请求不重发。恢复候选验证不能代替 active 切换。 |
| IP16 卸载 | NOT_RUN | 当前包卸载前排空、服务/程序/helper 清理，默认数据保留和独立核对；测试重置不代替默认卸载协议验收。 |
| IP17 对话续接 | UNVERIFIABLE | 旧版已有真实对话/项目绑定部分证据；当前候选仍须完整浏览器流/断线/附件/产物与重复提交、归档/跨 owner/workspace 负例。 |
| IP18 审批控制 | UNVERIFIABLE | 当前安装的 exact pending 展示、同 plan 批准/拒绝/取消/恢复及陈旧参数/nonce/Unknown 零重派；新文件链待窗口。 |
| IP19 项目监管 | UNVERIFIABLE | 已安装旧版有 workspace/context/charter 部分证据；当前版还需长期任务、新 decision/route、重连与 mid-run 不热换等联合验证。 |
| IP20 收件认知 | UNVERIFIABLE | 当前修复部署后的正式纠正/撤销/遗忘、原件/审计水位与真实本地/远端删除状态；旧 Unknown 预览不重试。 |
| IP21 设置维护 | UNVERIFIABLE | 完成受管消费者，当前安装的模型/渠道/能力/权限/网络/备份/升级真实状态；core 离线仍可受限维护。 |
| IP22 浏览器安全 | UNVERIFIABLE | 当前安装完整 Markdown/附件/产物、敏感缓存/退出、窄屏/键盘及事件负载与重连限流；源码/组件负例不替代正式浏览器。 |
| IP23 前端来源 | UNVERIFIABLE | 保留当前源码合规与精确白名单，再绑定当前安装的 UI digest/NOTICE/SBOM 和人工审查记录；不以旧 source receipt 代替当前包。 |
| IP24 联合回归 | NOT_RUN | 当前正式候选从认领到配置/真实对话/工具/项目/后台/重启/备份恢复/升级的一组贯通证据，并关联当前永久门。 |

## 4. 平台与工程交付

- Windows 11 x86_64 仍是当前实际安装环境。开发签名和 `release_ready=false` 不等于正式公网发布；不修改全机信任或 UAC 来伪造签名/权限结果。
- macOS arm64/x86_64、Ubuntu 24.04 x86_64/arm64、Rocky 9 x86_64 的原生 IP 仍未取得。owner 已选择 POSIX 有限恢复策略与不发布边界；不实现未批准的 one-shot wrapper，不用 WSL/Docker/交叉编译宣称支持。
- 维护规范、规模棘轮和固定 target 继续执行；已有 63 项历史规模债务不是零债务认证，也不自动刷新 baseline。空间不足不能删忽略配置/证据、跳测或降低 20 GiB 门槛。
- 只有本清单要求的实际状态与对应范围证据成立，才能关闭整体目标。Final PASS、单次安装、页面可达、真实模型回答或 typed unavailable 均不能单独作为整体完成依据。

## 5. 下一步顺序

1. 保存已结束的 Final 原始回执；回填本轮状态后只按现有规则刷新文档所影响的发布审计，不重复完整 workspace/Final，也不扩大审计排除集。
2. 三份冻结提案已获批准；正式文档与代码按依赖继续实施、验证和分别提交，不再要求同范围批准。
3. §7 的原失败窗口作为历史证据保留；后续授权重置测试安装与正式升级见现场启动修复记录。继续核验最新活动发布和原事务，不重派历史 Unknown，也不把组件回归当作已部署。
4. 完成剩余真实消费者和安装包后，按上述 IP 判据补齐原生联合证据及所需平台条件；环境不足保留未验证，不宣告整体完成。

## 6. 截图工件接线的追加源码证据

`protocol/installed_product/worker_result.rs` 已区分 Utf8/Binary，Host 的 `worker_results.rs::seal_path` 也封存 Binary 原件；但 `worker_backend.rs:288` 在完整长度/摘要核验后仍要求 UTF-8。因此即使补好 helper → Host 截图传输，原始 PNG 也不能直接沿当前主脑文本结果入口完成。`external_receipt` 把 result_ref 映射为 ContentRef，不证明模型或 Web 内容消费者已经接通。

这是源码核对，尚无安装版截图正向运行证据。后续契约必须同时闭合二进制工件、受保护读取与内容投影、精确动作绑定及中断恢复，不能通过有损转码、仅返回引用或删除验证来报完成。ContentRef/SecretRef 输入、真实 PTY、Shell/Browser/AppApi/Notification 的实际 placement 同样保留为实施义务；上述两份提案不自动授权额外未确定的输入/结果协议。完整核对在本地 `target/closure-inventory-evidence/installed-helper-io-gap-audit.md`，此处保存影响后续实施的结论。

## 7. 原生窗口取得回执后的状态更新

本地时间 10:29:58，原 launcher 的启动回执记录 `elevated_batch_launched`、PID 39760，绑定源码 `7b135294f024510de115c35e6fe91678e3c1e03b`。原窗口在 10:34:47 写入 stopped：阶段 `formal-owner-upgrade-once`，断言 `Original operation is unsettled; do not replay effects.`。独立产品汇总同为 stopped。原 launcher、consent 和该批次 PID 后续均不再存活，没有另起提权窗口。

`maintenance:correction-scope-stage-20261109` 已 committed、revision 30；其目标为 `forme-windows-x86_64-20261109-7b13529`。native 的 upgrade 和 identity_after 为空，matrix 数量为零，所以不能推断切换、健康检查、文件矩阵或数据窗口成功。随后只读 SCM 实测 Host/Web Running、Brain/updater Stopped；不能擅自把 Brain 再启动或重跑原升级来消除不明。

原件位于 `bootstrap-claim-journal/forme/target/native-legacy-review-20261106/`：

| 工件 | SHA-256 |
| --- | --- |
| `correction-scope-elevation-launch-20261109.json` | `43176d6d5c8f30fdf0d346c942762a16f5d4ebaf34ac2e296ea2c074bb5a0da4` |
| `correction-scope-native-window-20261109.json` | `f305cfe66aaeba3b483bbd6678093776d2d2d1073a135acd7d416ad8b9600876` |
| `correction-scope-product-window-20261109.json` | `1a249256061a424f4426709691bc7f0418fac5bbcad0ae7c2b1bd5d743faa3ff` |

接下来检查原 `maintenance:correction-scope-upgrade-20261109` 的 journal/receipt、备份与服务阶段，只沿原号判断是否执行过副作用及如何恢复。本轮普通 MCP I/O 的两个实现提交不在这个原生候选内，不能借原生窗口宣称它们已经部署。
