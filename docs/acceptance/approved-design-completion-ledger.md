# 已确认设计的整体交付清单

本清单承接 owner 2026-10-22 的指令：以**当前已确认设计全部实现并完成相应验收**为一个整体里程碑，持续推进，不以模块提交或组件门通过作为交接终点。它不增加 M6、内核、crate、依赖边、EventKind 或未批准功能。模块提交仍保留，便于审查和恢复。

权威范围为 requirements/01–18、architecture/canonical-contract §1–29、architecture/03–10 与 prd/01–26。历史 M0 薄化指引不能阻断 V1 已激活能力；明确的非目标和未来扩展位不因“所有文档”变成新增范围。旧验收回执只证明原 commit，不自动覆盖当前产品。

## 1. 状态与证据规则

状态分为：已有基线、实现中、待实现、待验证、环境受阻。整体完成必须同时有实现和要求层级的证据；模拟、组件、真实进程、浏览器、真实 provider/backend、原生平台和发布证据分别登记。环境受阻保留缺项，不折算为 PASS。

当前工作分支为 `shu/installed-product`，起点 `4367df7`；安装产品尚未合入 main。核心已有 main 基线，具体历史验收以 `core-brain-closure-acceptance-report.md` 为准，产品分支以 `installed-product-implementation-progress.md` 及其组件报告为准。

## 2. 核心与跨切面覆盖

| 已确认设计 | 代码/既有证据入口 | 本里程碑还须核实的内容 |
|---|---|---|
| prd/01–04：协议、事件单写者、运行/恢复、权限/审批 | protocol/store/harness/loop/policy/approval；S1–S22、M5 与 closure 门 | 产品入口保留当前绑定、持久幂等、Unknown 无盲重试；最终源码完整回归。 |
| prd/05/07/08/16：接入、能力、执行、模型 | capabilities/execution/models/config；S23–S99、C1–C24 | 受管设置接通正式 registry/driver；实际 provider 与隔离执行不得被界面模拟代替。 |
| prd/06/09–11：记忆、上下文、认知 | memory/context/cognition；C2/C7–C12 | Web 只读安全投影；候选、纠偏、撤回、遗忘接正式 governor/data lifecycle，清理 fence 同样有效。 |
| prd/12–15：协调、主动、沟通、验证 | coordination/harness/communication/eval；S/C 门 | 工作台接入项目、意图、建议/AskToLearn、审批、失败和 Unknown 的真实命令/事件。 |
| prd/17–22：合规、M1–M5 | compliance doctor、M5 release audit、前 97 taxonomy prefix | 产品包/前端/原生依赖新来源审计，历史证据不重写；当前源码重验。 |
| prd/23：默认主脑与 A/B/C/D | ProductionRuntime、99-kind/18-crate 门、closure release audit | CLI/Web/scheduler 同脑，产品更新/恢复不制造替代脑；当前源码/真实模型闭合。 |
| code-quality：工程维护 | 结构棘轮、strict Clippy、统一质量门 | 不增长既有规模债务；新增模块按责任拆分；全量验证与证据可追溯。 |

## 3. 安装、常驻与系统执行

| 设计/验收 | 已有实现 | 未闭合义务 |
|---|---|---|
| P01 / IP01 / prd24：完整原生包 | bundle、资源/签名验证、三平台模板、Windows 有限 ACL 安装入口 | 原生构建/签名/清单及失败恢复工具链；各目标实际安装证据。 |
| P02–03 / IP02–03：常驻、唯一主脑 | 三 daemon、SCM host、有限退避、core drain、独占 writer | 无桌面冷启动、故障/注销后任务持续、双启动与 Unknown 恢复原生证据。 |
| P09 / IP10：实际分权与 worker | 双域 broker、exact permit、持久 job receipt；普通进程生产 blocked；Windows 新安装 ACL | 逐 job 文件/网络/进程隔离、正式 core→broker driver、独立实际账户；每个平台有效拒绝。 |
| P09 / IP11：会话桥 | 核心 desktop grant/driver 治理基线 | 指定 OS 会话的有限 helper、TTL/liveness/锁屏/注销/TCC/Wayland 行为与平台证据。 |
| P10 / IP12–13：数据与接管 | AEAD/backup/restore/data lifecycle 核心；新脑认领；旧安装 template 原位保留 | 旧脑/备份离线验证、候选绑定、显式迁移与新路径切换、恢复材料；不接管错误 owner/key/占用库。 |
| P11 / IP14–15：升级回滚恢复 | product journal、release trust/compatibility、服务 adapter、阶段事实基础 | 签名 staging→排空→备份→切换→健康验证的完整事务；断电/磁盘失败恢复，不盲重放。 |
| P11 / IP16：卸载 | typed 停服/移除、默认保留数据的安装入口 | 原生卸载/网络与 helper 清理、准确范围/恢复说明；受阻时不继续删除程序。 |
| 平台账户/权限 | Windows 新布局组件验证；Linux/macOS 服务模板 | Linux/macOS 固定账户与权限预置、包生命周期、SELinux；平台未跑不扩大支持声明。 |

## 4. 认领、配置与网络

| 设计/验收 | 已有实现 | 未闭合义务 |
|---|---|---|
| P04–05 / IP04–05：无配置可达与认领 | 独立 Web、证明/CAS、有限 setup、新脑签名 claim、断线恢复 | 与整机首次安装连接；旧脑/备份候选与并发/中断的完整路径。 |
| P06 / IP06–07：账号与可信身份 | Argon2id、cookie/CSRF/epoch、plan-bound reauth、签名 bridge/durable fence | core 离线撤销恢复、OS 管理员有限口令恢复；账号/owner/keys 保持。 |
| P07 / IP08：持久设置 | 模型 patch/CAS/journal、不可变凭据、validate/start/restart/ack | 完整模型角色路由、workspace、能力/MCP/plugin/package、消息渠道、代理；真实测试仅经 core。 |
| P08 / IP09：本地与 LAN | 精确 loopback Host/Origin、分桶限流；LAN 未实现时拒绝 | TLS/certificate trust、网卡/Host/firewall/proxy typed 事务；跨设备证据与失败回退。 |
| P15 / IP21：离线维护 | 认证独立、host inspect/启停、模型配置与原 operation 核实 | 健康/诊断、备份候选、升级恢复/回滚、安全系统操作和完整状态视图。 |

## 5. Web 五个工作面

| 设计/验收 | 已有实现 | 未闭合义务 |
|---|---|---|
| P12 / IP17：对话 | 登录、提交/历史、cursor 续接、公开事件、幂等 | 会话创建/重命名/归档/项目关联，授权附件/产物；断线/双 tab 与 actual profile。 |
| P12/P14 / IP18：审批与控制 | 最终审批 binding、取消、运行状态 | 其他 PendingKind 的 typed 恢复、Unknown probe/裁决及真正终态确认。 |
| P13 / IP19：项目 | project/route register/plan/execute/cancel、charter | 意图/监管调整、历史/依赖/节点证据、刷新/恢复；不创建项目私有脑。 |
| P14 / IP20：收件箱 | `477ef52` 正式主动提案/AskToLearn/失败投影、问询回应与 waiting/failed 运行入口 | 普通建议完整采纳/拒绝/延后、Unknown 探测/裁决及完整浏览器验收；查看不等于采纳。 |
| P14 / IP20：记忆与认知 | `477ef52` 原始记忆引用、认知/候选/稳定/撤回/重评投影，证据/传递血缘及正式 review/撤回 | 授权原始正文、认知纠正/真正遗忘与完整 IP20；现有组件证据不等于产品通过。 |
| P07/P15 / IP21：设置 | 模型、口令、主脑状态、启动/ack | 完整分区的正式命令与维护；无假开关/任意 shell/SQL。 |
| P08/P12 / IP22：浏览器安全 | CSP、无 raw HTML、独立限流、private cache 清理 | 附件/下载隔离、实际浏览器故障/恶意内容、键盘/窄屏与认证恢复验收。 |
| P16 / IP23：前端来源 | 自有 React/TypeScript UI，白名单/依赖/NOTICE/资源构建门 | 新组件继续来源审查；无 upstream runtime、fixture 或 build/import。 |
| P01–16 / IP24：完整产品 | 当前只有分层组件证据 | 正式包→认领→配置→真实对话/工具审批→项目/后台→重启→备份恢复→升级的联合验收。 |

## 6. 持续推进与最终门

实施依赖按底层契约/正式 API→Web 操作→产品事务/原生适配→联合验证推进；可独立工作并行准备，但不创建另一套配置、owner、任务状态或事实源。发现额外已批准义务时补入本清单，不隐去难项。每次新增功能必须更新相应证据，组件通过后继续后续义务。

最终必须核对全部 P/IP、S1–S99、相关 C1–C24、冻结 graph/taxonomy、strict Clippy、质量棘轮、完整默认合规、typed artifacts 与当前源码 release audit；main 集成与远端发布按既有授权分别处理。

当前 Windows 会话不是管理员，未发现 Inno 编译器；Linux/macOS 原生目标与签名身份未提供。先完成可实现的代码/工具与本机证据，不伪造原生支持、签名或管理员安装结论；这些限制只阻断依赖它们的验收，不阻断其余工作。
