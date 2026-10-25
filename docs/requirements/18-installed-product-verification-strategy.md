# 常驻服务与 Web 工作台验证策略

本策略承接 [P01–P16](17-installed-product-scope-decisions.md)，定义产品化的独立 IP01–IP24 验收。**已有分阶段组件实现与测试，尚无完整产品化 IP PASS 报告。** 当前进度见 `../acceptance/installed-product-implementation-progress.md`。历史 S1–S99、C1–C24、18-crate 图、99 EventKinds 和 single authority 继续是不可回退基线。

## 1. 证据标准

每个 IP 场景至少保存：源码 commit、包/UI 摘要、OS/架构/浏览器版本、脱敏配置来源、前置数据基线、操作步骤、实际结果、独立探测、反向断言、残余限制。使用 `PASS / FAIL / UNVERIFIABLE / NOT_RUN / NOT_APPLICABLE`，不可验证不计通过。报告不得保存账号密码、cookie、bootstrap token、provider key、私有路径或可利用的 SecretRef。

服务状态必须由 OS 服务管理器和进程/端口/任务进展共同证明；不能只看安装器退出码。UI 必须有真实浏览器交互、后端事件与状态恢复证据，截图不证明执行效果。模型有用性另保留真实模型/工具与简单基线，不让 UI 冒充能力增长。

## 2. 场景矩阵

| 场景 | 对应需求/负责 PRD | 正向验收 | 必须同时成立的反向断言 |
|---|---|---|---|
| IP01 原生整体安装 | P01 / 24 | 在无 Rust/Node 的目标系统安装，校验包签名、资源完整性、服务身份与固定地址；无桌面主机可完成 bootstrap。 | 错误 OS/CPU、损坏包、无效签名、端口冲突不留下假 Ready。 |
| IP02 无登录常驻 | P02 / 24 | 冷启动后不登录桌面，服务可达；关闭全部网页、Web 退出及 OS 注销后，后台授权任务仍产生新事件。 | 休眠/关机时不虚报持续执行；服务失败不是“登录后才恢复”。 |
| IP03 崩溃与恢复 | P02/P03 / 24 | 终止核心进程后 OS 按上限恢复，同一 owner/store 恢复；Web 故障不停止 core。 | 每个进程只有一个重启监管者；双启动零重复 writer/任务；Unknown 不重做。 |
| IP04 未配置可达 | P04/P15 / 25 | 清洁安装无模型/owner config，Web 显示初始化；配置错误或核心离线时维护页面仍可登录。 | 不启动空 context 大脑，不把错误配置覆盖成空库，不开放匿名管理。 |
| IP05 抢占防护 | P05 / 25 | 本地证明换有限 setup、验证/选择脑后认领，事务中断可恢复；headless 通过本地终端/SSH tunnel 完成。 | LAN 先访问、重放证明、并发认领、过期证明、setup 越权、日志/URL 泄露均不能夺取 owner。 |
| IP06 登录与恢复 | P06 / 25 | 登录、过期、退出、全会话撤销、改密码和 OS 管理员恢复均可验证，OwnerId 与脑数据不变。 | 错误密码不泄露账户存在性；恢复不要求删除脑，不丢失数据解密能力。 |
| IP07 入口身份绑定 | P03/P06 / 25 | Web 验证登录后经内部认证绑定 AuthContext，core 审计记录真实 owner/会话来源。 | payload 伪造 principal、跨 owner、直连内部 Gateway 或伪造 forward header 均被拒，driver=0。 |
| IP08 配置生效 | P07 / 25 | 保存→校验→应用有版本与结果；重启后仍加载同一 profile；记录实际 provider；环境覆盖可见。 | 保存失败不覆盖好配置；改模型不改变治理；密钥不回显、不进日志/事件/诊断包。 |
| IP09 LAN 边界 | P08 / 25 | 默认仅本地；启用 LAN 后正确 HTTPS/Host/Origin/firewall，跨设备登录可用。 | HTTP、未信任证书、DNS rebinding、CSRF、错误 Host、未授权网卡不被静默放行；不默认开公网。 |
| IP10 权限分离 | P09 / 24 | 检查服务账户/ACL；经审批 typed system action 可执行且有完整事件/permit/receipt。 | 模型、Web、shell worker 无权改服务二进制/配置/brain DB/keys；缺权限、陈旧审批、重放许可 driver=0。 |
| IP11 交互会话桥 | P09 / 24 | 真实登录会话 helper 绑定 OS 用户、session generation、desktop grant、exact plan/permit 与受保护 peer；许可有效时完成一次桌面动作并返回原 job receipt。 | 注销、锁屏、TCC/portal 拒绝、会话切换、过期/重放 nonce、参数或 plan digest 替换、helper 失联均阻断；`Started` 无终态进入 Unknown；不退回高权限服务直接点桌面。 |
| IP12 数据与密钥 | P10 / 24/25 | 绝对路径、服务 ACL、应用层加密、无交互重启读写、备份与恢复演练一致。 | 普通执行账户不能读原文/keys；改密码或注销不丢密钥；敏感数据不进静态资源/缓存。 |
| IP13 旧脑接管 | P04/P10 / 24/25 | 检测历史配置/owner/store；首次认领前绑定已验证原脑/恢复候选，备份、显式迁移、保留原件与切换证据。 | 错误 key、历史明文、owner 不匹配、占用库均不能新建空脑替代；不能先造 owner 再暗换身份，不能修饰历史保护状态。 |
| IP14 升级事务 | P11 / 24 | 签名→预检→排空→一致性备份→切换→健康探测→恢复任务；升级中 Web 显示维护进度。 | 断电/磁盘满/迁移失败可定位阶段；不覆盖旧好版本，不丢事件、不重复外部动作。 |
| IP15 回滚与恢复 | P10/P11 / 24 | 兼容 binary rollback；备份恢复新路径校验后 owner 切换，旧路径可追溯。 | 不兼容 schema 禁止二进制盲回滚；备份无 key 时明确不可恢复；恢复不自动重发历史请求。 |
| IP16 卸载 | P11 / 24 | 停止任务接入、移除服务/程序/firewall 和会话桥，默认保留脑/备份与可读恢复说明。 | 不误删 owner workspace/原始资料；“删除数据”须另有明确范围，不随默认卸载发生。 |
| IP17 对话与续接 | P12 / 26 | 浏览器提交、流式输出、刷新和断线续接，按 authoritative cursor 折叠；附件和产物可查；会话关联 workspace 后，新 run 的 `SessionBound` 与正式 context 均绑定该 workspace。 | 双击/断网重提不双跑；事件 gap 可恢复；伪造 workspace、跨 owner/归档 session、无效项目均在模型/driver 前拒绝；关联并发变更只影响后续新 command；未知结果使用原 workspace/key 核实；页面不直调模型，不显示虚构进度/思维链。 |
| IP18 审批与控制 | P09/P12/P14 / 25/26 | 页面展示最终动作/参数/资源/风险，批准或拒绝绑定 exact pending；取消/恢复有确认状态。 | 陈旧页面、参数替换、nonce 重放、退出后重放、Unknown 误重试均零额外副作用。 |
| IP19 项目监管 | P13 / 26 | 编辑 charter、看目标/历史/长期任务，调整后主脑生成新 decision/route，重连后保持。 | 不创建 workspace 私有 cognition；不能 mid-run 换 route 或用 UI 直接改稳定认知。 |
| IP20 收件箱与认知 | P14 / 26 | 主动建议、问题、审批、失败、Unknown 有来源和处置；纠正/撤销/遗忘在正式事件投影可验证。 | 浏览/已读不等于采纳，候选不自动稳定，删除显示本地/远端真实状态。 |
| IP21 设置与维护 | P07/P15 / 25/26 | 模型、channels、能力、权限、网络、备份、升级和健康均显示实际状态/版本；core 离线可修配置。 | 安装≠启用≠授权≠可信；维护页面不提供任意 SQL/shell/文件编辑或第二 event writer。 |
| IP22 浏览器安全与负载 | P08/P12 / 25/26 | Markdown/附件隔离、下载授权、敏感缓存隔离、键盘操作与窄屏；长连接重连退避正常。 | XSS/路径穿越/未授权 artifact 拒绝；事件读取不耗尽命令/登录限流，不再出现轮询 429 风暴。 |
| IP23 前端来源边界 | P16 / 26/17 | 固定 upstream revision、文件/依赖/许可证清单、NOTICE、SBOM、构建隔离及人工 review。 | Cordis/上游服务端/插件/调度/认知/fixture 不进入构建；无 blanket copy allowlist。 |
| IP24 完整产品回归 | P01–P16 / 24–26 | 从正式安装包完成认领→配置→真实对话/工具审批→项目→后台→重启→备份恢复→升级；复跑原永久门。 | 开发服务器/mock/跨编译/旧 release receipt 不冒充已安装原生全链路 PASS。 |

## 3. 平台和实现顺序

按 **安装与运行底板（prd/24）→ 初始化/认证/接入（prd/25）→ Web 工作台（prd/26）→ IP24 联合验收** 推进。交互契约先联调，不能独立建三套配置、owner 或任务状态。

| 原生验收目标 | 必做 | 不能替代的证据 |
|---|---|---|
| Windows 11 x86_64 | 安装/SCM/无用户登录/ACL/桌面会话桥/升级卸载。 | 手动运行 exe 或 Scheduled Task 不等于 SCM。 |
| macOS arm64、x86_64 | 每个发布架构的签名公证包、LaunchDaemon、TCC 与无桌面启动。 | 交叉编译和另一个 CPU 的测试不证明本架构可用。 |
| Ubuntu 24.04 x86_64、arm64 | `.deb`、systemd、专用用户、无图形环境安装、重启、恢复。 | Docker/WSL 中运行不能替代原生 service acceptance。 |
| Rocky Linux 9 x86_64 | `.rpm`、systemd、SELinux 与 firewall 规则、升级卸载。 | `.deb` 通过不证明 RPM/SELinux 路径。 |

未能取得的平台记录 NOT_RUN/UNVERIFIABLE，不扩大支持声明。平台能力可明确降级，例如 headless host 没有交互桌面；基础安装、认证、对话和维护不能因此被标成不适用。

### IP11 交互会话桥的证据细则

正向证据必须同时包含：真实 OS 用户与 session generation 的独立探测、Host/helper peer 身份、签发的 desktop grant 与 TTL、原 `WorkerJob`/`WorkerPermit`/`plan_digest`/nonce 绑定、一次真实有限桌面动作、helper receipt 与 core 终态的对应关系。仅能证明 helper 进程运行、服务拥有管理员权限或动作在 mock driver 中成功，不算正向通过。

反向证据至少覆盖：未登录、锁屏、注销、会话切换、Windows 非目标 session、macOS TCC 拒绝、Linux Wayland portal/X11 authority 缺失、过期 grant、nonce 重放、参数/digest 替换、Web 直连 helper、helper 失联以及动作已 started 但没有终态。每个反向场景都必须证明 driver/桌面副作用次数为零；最后一种必须证明状态唯一为 `Unknown` 且恢复不产生第二 dispatch。

## 4. 三类联合正确性

- **Runtime**：OS 生命周期、唯一运行时、command 幂等、事件连续性、断线/崩溃/升级/磁盘故障可恢复。
- **Governance**：登录只建立入口身份，所有真实动作仍经最终 plan recheck；分权、CSRF、审批、未知副作用与 owner 数据安全均有否定测试。
- **Learning/value**：Web 只呈现已有认知链，真实任务继续核验 memory/candidate/反思/长期监管回流；UI 成功不能证明越用越聪明。

安装、服务、网络与认证审计使用产品管理记录；大脑行为使用既有 EventKind/stream_seq；两者用安全 operation/run refs 关联，不合并为第二事实源。诊断/验收工件只保留脱敏摘要与校验值。

## 5. 发布阻断

任何 owner 抢占、匿名管理、越权、密钥泄漏、重复副作用、双 writer、数据损坏、历史明文保护虚报、证书信任虚报或 upstream 边界突破都阻断发布，不能用总体通过率抵消。签名、原生系统可用性与操作文档缺失时只能交付受限测试版本，并明确范围。

产品化 release audit 必须绑定本次安装器、Rust binaries、静态 UI、来源/SBOM 与测试版本。新增功能改变源码或包输入后，旧验收只能作为历史记录，不得仅换摘要获得新 PASS。文档完成不生成安装验收报告。
