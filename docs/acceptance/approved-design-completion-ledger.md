# 已确认设计的整体交付清单

本清单承接 owner 2026-10-22 的指令：以**当前已确认设计全部实现并完成相应验收**为一个整体里程碑，持续推进，不以模块提交或组件门通过作为交接终点。它不增加 M6、内核、crate、依赖边、EventKind 或未批准功能。模块提交仍保留，便于审查和恢复。

权威范围为 requirements/01–18、architecture/canonical-contract §1–29、architecture/03–10 与 prd/01–26。历史 M0 薄化指引不能阻断 V1 已激活能力；明确的非目标和未来扩展位不因“所有文档”变成新增范围。旧验收回执只证明原 commit，不自动覆盖当前产品。

## 1. 状态与证据规则

状态分为：已有基线、实现中、待实现、待验证、环境受阻。整体完成必须同时有实现和要求层级的证据；模拟、组件、真实进程、浏览器、真实 provider/backend、原生平台和发布证据分别登记。环境受阻保留缺项，不折算为 PASS。

产品主施工分支为 `shu/installed-product`；受管 skills、公共设置 v2、网络健康与回退增量已合入该分支，尚未合入 `main` 或发布。提交与工作树状态以当前 Git 状态为准；核心 main 基线与历史验收见 `core-brain-closure-acceptance-report.md`，产品分支状态见 `installed-product-implementation-progress.md` 及组件报告。会话产物历史、运行未知结果 owner 裁决恢复、会话 workspace 到后续正式 run 的上下文绑定均已有实现/组件证据；真实浏览器、多进程和整机联合验收仍未完成。

## 2. 核心与跨切面覆盖

| 已确认设计 | 代码/既有证据入口 | 本里程碑还须核实的内容 |
|---|---|---|
| prd/01–04：协议、事件单写者、运行/恢复、权限/审批 | protocol/store/harness/loop/policy/approval；S1–S22、M5 与 closure 门 | 产品入口保留当前绑定、持久幂等、Unknown 无盲重试；最终源码完整回归。 |
| prd/05/07/08/16：接入、能力、执行、模型 | capabilities/execution/models/config；S23–S99、C1–C24；受管 skills 组件报告 | skills 已接入正式 `SkillRegistry` 与 ProductionRuntime；MCP/plugin/package/driver、实际 provider 与隔离执行仍不得被界面模拟代替。 |
| prd/06/09–11：记忆、上下文、认知 | memory/context/cognition；C2/C7–C12 | Web 只读安全投影；候选、纠偏、撤回、遗忘接正式 governor/data lifecycle，清理 fence 同样有效。 |
| prd/12–15：协调、主动、沟通、验证 | coordination/harness/communication/eval；S/C 门 | 工作台接入项目、意图、建议/AskToLearn、审批、失败和 Unknown 的真实命令/事件。 |
| prd/17–22：合规、M1–M5 | compliance doctor、M5 release audit、前 97 taxonomy prefix | 产品包/前端/原生依赖新来源审计，历史证据不重写；当前源码重验。 |
| prd/23：默认主脑与 A/B/C/D | ProductionRuntime、99-kind/18-crate 门、closure release audit | CLI/Web/scheduler 同脑，产品更新/恢复不制造替代脑；当前源码/真实模型闭合。 |
| code-quality：工程维护 | 结构棘轮、strict Clippy、统一质量门 | 不增长既有规模债务；新增模块按责任拆分；全量验证与证据可追溯。 |

## 3. 安装、常驻与系统执行

| 设计/验收 | 已有实现 | 未闭合义务 |
|---|---|---|
| P01 / IP01 / prd24：完整原生包 | bundle、资源/签名验证、三平台模板、Windows 有限 ACL 安装入口；`277f551` 加固 POSIX 包事务的固定环境、root/systemd 前置检查与卸载 fixture | 原生构建/签名/清单及失败恢复工具链；各目标实际安装证据。 |
| P02–03 / IP02–03：常驻、唯一主脑 | 三 daemon、SCM host、有限退避、core drain、独占 writer | 无桌面冷启动、故障/注销后任务持续、双启动与 Unknown 恢复原生证据。 |
| P09 / IP10：实际分权与 worker | 双域 broker、exact permit、持久 job receipt；普通进程生产 blocked；Windows 新安装 ACL/Service SID；`1ae55c6` 正式 core→worker/加密结果/双向 peer，`cc43440`、`1270e54`、`7cb5b8b` 有限文件入口、首装 profile/workspace 装配与活动程序绑定，详见本批文件 worker 组件报告。 | 原生 SCM/LocalSystem 安装后的账户、文件/网络/进程隔离及拒绝证据；每个平台独立验证。交互 Host/AppContainer 与临时进程不等于 IP10 PASS。 |
| P09 / IP11：会话桥 | 核心 desktop grant/driver 治理基线 | 指定 OS 会话的有限 helper、TTL/liveness/锁屏/注销/TCC/Wayland 行为与平台证据。 |
| P10 / IP12–13：数据与接管 | AEAD/backup/restore/data lifecycle 核心；新脑认领；旧安装 template 原位保留 | 旧脑/备份离线验证、候选绑定、显式迁移与新路径切换、恢复材料；不接管错误 owner/key/占用库。 |
| P11 / IP14–15：升级回滚恢复 | product journal、release trust/compatibility、服务 adapter；`5b99ac5` 已实现恢复水位、活动数据 CAS、完整 CommitRestore 与提交后准入；`e36b5b9` 增加独立 Web 激活与已暂存发布的重新认证 | 恢复的原生 SCM 联调；签名 Upgrade/Rollback/self-update 的排空→备份→程序切换→健康验证与断电/磁盘失败恢复。不以恢复组件/禁用状态截图代替原生升级验收。 |
| P11 / IP16：卸载 | typed 停服/移除、默认保留数据的安装入口 | 原生卸载/网络与 helper 清理、准确范围/恢复说明；受阻时不继续删除程序。 |
| 平台账户/权限 | Windows 新布局组件验证；Linux/macOS 服务模板 | Linux/macOS 固定账户与权限预置、包生命周期、SELinux；平台未跑不扩大支持声明。 |

## 4. 认领、配置与网络

| 设计/验收 | 已有实现 | 未闭合义务 |
|---|---|---|
| P04–05 / IP04–05：无配置可达与认领 | 独立 Web、证明/CAS、有限 setup、新脑签名 claim、断线恢复 | 与整机首次安装连接；旧脑/备份候选与并发/中断的完整路径。 |
| P06 / IP06–07：账号与可信身份 | Argon2id、cookie/CSRF/epoch、plan-bound reauth、签名 bridge/durable fence | core 离线撤销恢复、OS 管理员有限口令恢复；账号/owner/keys 保持。 |
| P07 / IP08：持久设置 | 模型 patch/CAS/journal、不可变凭据、validate/start/restart/ack；`0990bd0` 接入默认 workspace、前台优先与后台相关性模型；受管 skills、公共设置 v2、受管模型代理与可用性投影已接入正式 `SkillRegistry`/ProductionRuntime，组件见 `installed-product-managed-skills-acceptance.md` 与 `installed-product-model-proxy-acceptance.md`。 | MCP/plugin/package、消息渠道、真实外部 provider 和完整产品验收仍仅经 core；组件通过不单独提升 IP08。 |
| P08 / IP09：本地与 LAN | 精确 Host/Origin、分桶限流；受管网络投影、TLS 材料校验、公共设置 v2、监听健康/有界回退、单次维护派发和 Windows 管理员 firewall 规则的只读核验已接入，见网络组件报告。 | 其他平台核验、真实管理员规则/SCM、证书客户端信任及跨设备证据；不自动改防火墙。 |
| P15 / IP21：离线维护 | 认证独立、host inspect/启停、模型配置、原 operation 核实；`1270e54` 的固定 hostctl 离线备份核验与升级/回滚 Web 控件，`7cb5b8b` 绑定已验证活动发布，见[组件报告](installed-product-file-worker-and-offline-maintenance-20261027.md)。 | 原生服务、实际升级/恢复/回滚、完整维护状态及产品联合证据；前端 fixture 不等于真实维护事务。 |

## 5. Web 五个工作面

| 设计/验收 | 已有实现 | 未闭合义务 |
|---|---|---|
| P12 / IP17：对话 | 登录、提交/历史、cursor 续接、公开事件、幂等；会话元数据 CRUD；加密附件上传、授权产物目录/下载；pending run 的 typed owner 裁决、密文伴随记录与重启后原命令核实；会话 workspace 绑定到后续正式 run 的 `SessionBound` 上下文（组件报告）。 | 真实浏览器断线/双 tab、多个 Web 进程争用和实际 provider；完整 IP17 产品验收。 |
| P12/P14 / IP18：审批与控制 | 最终审批 binding、取消、运行状态；approval/tool/handoff pending 投影、Unknown 核实与 typed owner 裁决实现。 | 真实浏览器/多进程恢复和原生服务重启场景；不得将组件测试折算为 IP18/IP24 通过。 |
| P13 / IP19：项目 | project/route register/plan/execute/cancel、charter | 意图/监管调整、历史/依赖/节点证据、刷新/恢复；不创建项目私有脑。 |
| P14 / IP20：收件箱 | `477ef52` 主动提案/AskToLearn/失败投影和处置；运行 pending 按 `RunPendingView` 投影为 typed `Waiting` 并导航原 run；`507cccf` 将普通建议未决原命令保存到独立身份库，支持重启读回和显式原号核实，组件见 `installed-product-proposal-recovery-acceptance.md`。 | 真实浏览器刷新/双 tab、多 Web 进程、服务重启全链路与完整 IP20。等待/失败历史必须区分，查看不等于采纳。 |
| P14 / IP20：记忆与认知 | `477ef52` 投影、证据/血缘及 review/撤回；`de3c7b1` 来源正文核对；`91072be` 具体来源的真实遗忘；`802e3d2` 原子文字纠正、旧认知失效/低置信候选、无正文回执与删除后不复活，见 `installed-product-cognition-correction-acceptance.md`。临时真实浏览器已验证纠正、刷新与原号查询。 | 双 tab/多 Web 进程、实际 provider、原生重启及完整 IP20/IP24 仍开放。组件、临时浏览器和本机模型 fixture 不等于产品通过。 |
| P07/P15 / IP21：设置 | 模型配置、默认 workspace 与模型角色、受管 skills、口令、主脑状态、受管网络/TLS 与受限启停/维护命令；配置、离线数据、恢复候选和活动发布各有组件报告。 | 真实 backend/provider、LAN 跨设备/证书信任、原生系统维护和完整 IP21 产品证据仍开放；MCP/plugin/package/消息渠道按 PRD25 §11.10–11.11 显示实际 unavailable，无假开关/任意 shell/SQL。 |
| P08/P12 / IP22：浏览器安全 | CSP、无 raw HTML、独立限流、private cache 清理；`5391750` 的真实临时浏览器组件证据覆盖 320/390 窄屏、键盘导航与两标签页注销清除，见 `installed-product-browser-component-acceptance-20261026.md`。 | 附件/下载隔离、实际浏览器故障/恶意内容、完整认领与业务恢复验收；预认领/core 离线组件不提升完整 IP22/IP24。 |
| P16 / IP23：前端来源 | 自有 React/TypeScript UI，白名单/依赖/NOTICE/资源构建门 | 新组件继续来源审查；无 upstream runtime、fixture 或 build/import。 |
| P01–16 / IP24：完整产品 | 当前只有分层组件证据 | 正式包→认领→配置→真实对话/工具审批→项目/后台→重启→备份恢复→升级的联合验收。 |

## 6. 持续推进与最终门

实施依赖按底层契约/正式 API→Web 操作→产品事务/原生适配→联合验证推进；可独立工作并行准备，但不创建另一套配置、owner、任务状态或事实源。发现额外已批准义务时补入本清单，不隐去难项。每次新增功能必须更新相应证据，组件通过后继续后续义务。

最终必须核对全部 P/IP、S1–S99、相关 C1–C24、冻结 graph/taxonomy、strict Clippy、质量棘轮、完整默认合规、typed artifacts 与当前源码 release audit；main 集成与远端发布按既有授权分别处理。

当前 Windows 会话不是管理员；固定摘要的 Inno 6.6.1 已基于实现提交 `7cb5b8b` 重新编译六程序 Windows 未签名、未安装开发候选，详见[2026-10-27 候选报告](installed-product-windows-candidate-20261027.md)；2026-10-24/26 候选保留为历史。Linux/macOS 原生目标与签名身份未提供。继续完成可实现的代码/工具与本机证据，不伪造原生支持、签名或管理员安装结论；这些限制只阻断依赖它们的验收，不阻断其余工作。

安装 worker 的有界结果读取/Host manifest-chunk 通道，以及 POSIX 有限服务恢复的兼容变更，见 `architecture/proposals/installed-worker-result-and-posix-recovery.md`。owner 已批准结果通道并选择 POSIX 方案 A。`1ae55c6` 的 ProductionRuntime adapter、应用层 AEAD、结果核验与双向 peer 之后，`cc43440`、`1270e54`、`7cb5b8b` 已补齐有限文件入口、首次安装的 profile/workspace/capability 装配与活动发布程序定位，证据见[文件 worker 与离线维护报告](installed-product-file-worker-and-offline-maintenance-20261027.md)。原生 SCM/LocalSystem、桌面 helper、正式安装及完整 IP 仍开放。下一项权限边界已形成[具体草案](../architecture/proposals/installed-windows-desktop-helper-native-boundary.md)，尚未扩大 unsafe 白名单；本机安装按[原生验收计划](installed-product-windows-native-acceptance-plan.md)另行批准。Linux/macOS 保持 `release_ready=false`，不宣称原生产品完成。
