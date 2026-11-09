# 安装产品剩余实施与原生验收义务审计（2026-11-09）

本清单以当前 `shu/closure-inventory@6899f56` 和真实安装状态区分实现、源码回归、安装验收。本轮完整源码 Final 已取得 exit 0，详见[核心终验记录](core-brain-closure-acceptance-report.md#7-纠正-scope-与闭合清单后的完整源码终验2026-11-09)。本审计不是安装或发布声明，不把局部成功提升成整个场景 PASS。原始范围保持为 requirements/17 P01–P16、requirements/18 IP01–IP24、PRD23 C1–C24 与历史 S1–S99，以及工程维护规则。

## 1. 已有证据及正在执行的原号

| 层级 | 已取得的证据 | 当前边界 |
| --- | --- | --- |
| 当前整仓源码 | `6899f56` 的完整 Final 实际 exit 0；183 个 Rust test targets：1,310 passed / 0 failed / 16 默认 ignored，strict Clippy、完整 compliance、Python 94 passed / 1 skipped。 | 默认 ignored 不计通过；两次 workspace 阶段不叠加计数。该分支未合入 main。 |
| 当前四波 | Final 已逐名复验 A/B/C/D 47/50/36/73，共 206 项，并通过便携工件与 typed verifier。 | 两个新增 C23 场景在 D 清单内；C24 真实证据保留原采集和源码绑定，不宣称本轮再次调用真实模型。 |
| C24 真实模型 | 六类工作流、各两轮，primary 与 baseline 均有真实 profile 和评分证据；unknown cost 保留。 | 已验证该组任务，不证明长期收益或安装版 IP，更不声称 forme 质量胜过直接模型。 |
| 当前审计回执 | core `7bcc6fac780a1c1669f8dd527e4e283cdbbfc11ea140ee7086fb8c676c047069`；M5 `78eb9f2e29a19ddaac0abe638e082bdd1e871953b77b646a18a5acac4dabfb80`。 | Final 内重新生成、独立 verify 与当前干净树 compare 均 PASS；这些是本次 `6899f56` 的回执。文档记录提交后，规范回执须按现有规则重新绑定。 |
| 原生安装 | 已安装版本 `forme-windows-x86_64-20261107-cf3fbc9`；Host/Web/Brain 为 SCM Running，updater Stopped。 | 服务运行不是新版升级证据。当前 Product URL 为本机 loopback。 |
| 新原生候选 | `forme-windows-x86_64-20261109-7b13529` 的签名、封闭文件集、PE 与输入清单已准备。 | 原生批次未取得管理员启动回执；仍是同一个待 UAC 的进程，不重新派发。 |

Final 为原 `target/closure-inventory-evidence/run-current-final.ps1` 会话，实际在本地时间 03:14:03–04:11:07 结束；exit 0、`core-brain-closure (Final): PASS`、clean tree、发布回执 compare 与末次空间报告均成立。`current-final.log` SHA-256 为 `4ab96146e99c5acafe843ea7b1d7a01323c1d74d7a3589812013aef3c4ceb080`。后续验收说明和回执提交不冒充在新 commit 上重新运行完整 Final。

原生批次保留于 `bootstrap-claim-journal` 工作树，源码固定 `7b13529`；目标为一次签名升级、文件 deny/write/read/delete、备份恢复负例与受控 StopCore/StartCore。当前缺 `correction-scope-elevation-launch-20261109.json`、`correction-scope-native-window-20261109.json`、`correction-scope-product-window-20261109.json`。原 Unknown command 只查原件，不重新派发。

## 2. 尚需实现的安装版能力

| 义务 | 当前缺口及下一项可执行交付 | 决策状态 |
| --- | --- | --- |
| 既有能力的受管配置及消费者 | settings v3、Host 注册来源/程序/包公钥，MCP 隔离 discovery/call、插件贡献、已有渠道 adapter 与 typed M5 Web 控制。 | `docs/architecture/proposals/installed-managed-consumers-closure.md` 已可审查；等待明确契约批准，未改冻结 DTO/trait。 |
| 桌面授权及主脑接线 | Host OS 会话目录、pending/consent/issue/revoke journal，exact grant 消费、正式 planner/backend 与 Web 会话选择。 | `docs/architecture/proposals/installed-desktop-authority-closure.md` 已可审查；等待明确契约批准。 |
| 截图、引用输入和真实 PTY | helper 当前拒绝截图和 ContentRef/SecretRef Type，只处理 Computer；desktop 结果 source=None。`1207740` 已证明测试专用 Low AppContainer/ConPTY、后代取消与原文件派生限制三个机制；I/O 故障矩阵及生产接线未完成。 | 组件结果见[PTY 机制验收](installed-isolated-pty-mechanism-acceptance-20261109.md)。仍须闭合受保护输入/结果、Host grant、生产 placement 和真实安装证据；不凭机制或两份提案自动授权新增协议。 |
| 其他安装版执行 placement | Shell、Browser、AppApi、Notification 逐项核对实际消费者、隔离 profile、凭据/输入/结果与取消。 | 有通用 broker/factory 分支不等于实际安装能力；不得统一启用 flag 或回退 brain 进程内 driver。 |

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
| IP10 权限分离 | UNVERIFIABLE | 当前候选真实 SCM 身份与 ACL、隔离 worker 正向终态及审批/permit/replay 零额外副作用。新四项文件矩阵尚待原生窗口。 |
| IP11 桌面会话 | UNVERIFIABLE | 完成 Host 授权与正式主脑接线后，真实 session/helper/peer/action/receipt；锁屏/注销/换会话/撤权/失联分别取证。 |
| IP12 数据密钥 | UNVERIFIABLE | 当前安装真实加密/ACL、无交互重启、备份恢复和口令变化不丢解密能力；新备份/离线窗口尚待执行。 |
| IP13 旧脑接管 | UNVERIFIABLE | 真实安装的三类候选、独占锁/keys、catalog→claim→activation→runtime ack 全链及错误来源负例；隔离协议测试不足。 |
| IP14 升级事务 | UNVERIFIABLE | 原号签名升级完成排空/备份/切换/健康、原身份事件保持，以及失败阶段的可恢复证据。当前新候选没有启动回执。 |
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
2. 已写好的两份冻结提案待明确答复；批准后先回填正式文档，再按依赖实现，模块各自验证/提交。
3. 原生窗口只沿原请求取得 OS 管理员令牌后执行；不叠加 RunAs，不重新派发旧 Unknown，不伪造缺失回执。
4. 完成剩余真实消费者和安装包后，按上述 IP 判据补齐原生联合证据及所需平台条件；环境不足保留未验证，不宣告整体完成。

## 6. 截图工件接线的追加源码证据

`protocol/installed_product/worker_result.rs` 已区分 Utf8/Binary，Host 的 `worker_results.rs::seal_path` 也封存 Binary 原件；但 `worker_backend.rs:288` 在完整长度/摘要核验后仍要求 UTF-8。因此即使补好 helper → Host 截图传输，原始 PNG 也不能直接沿当前主脑文本结果入口完成。`external_receipt` 把 result_ref 映射为 ContentRef，不证明模型或 Web 内容消费者已经接通。

这是源码核对，尚无安装版截图正向运行证据。后续契约必须同时闭合二进制工件、受保护读取与内容投影、精确动作绑定及中断恢复，不能通过有损转码、仅返回引用或删除验证来报完成。ContentRef/SecretRef 输入、真实 PTY、Shell/Browser/AppApi/Notification 的实际 placement 同样保留为实施义务；上述两份提案不自动授权额外未确定的输入/结果协议。完整核对在本地 `target/closure-inventory-evidence/installed-helper-io-gap-audit.md`，此处保存影响后续实施的结论。
