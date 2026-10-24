# 默认工作空间与模型角色配置组件验收

日期：2026-10-24。分支：`shu/installed-product`。设计提交 `65910cc`，实现提交 `0990bd0`。承接 `prd/25` 的持久配置和 `prd/26` 设置工作面；本报告仅记录组件、真实临时进程与有限浏览器检查，不提升 IP08/IP21/IP24 或原生安装状态。

## 1. 实现与边界

正式路径为 Web 配置 → Host 的配置 CAS/journal → 不可变配置 revision/core.json → ManagedBrainConfig → 唯一 ProductionRuntime。新增默认工作空间、前台兼容模型优先项和后台相关性判断模型；Web 不调用模型或直接写脑状态。已单独绑定的会话/项目和既有 run 保留原 workspace。

- 偏好和模型清单共享同一次 operation、revision、写者锁与发布恢复边界；未知保存保留原 key/body，显式核实，不自动重试。
- 省略偏好的旧客户端保持已保存值；未配置时使用安装模板，并在页面显示有效来源。旧 operation digest 与历史 core materialization 保持原字节语义，不原地重写历史。
- 前台优先项不能放松工具能力、上下文和风险条件。后台显式角色必须绑定实际已注册 provider；缺少其凭据时拒绝启动，不悄悄继承其他 profile 的身份。实际调用事件记录实际 profile。
- 保存、读取、静态校验不调用模型。配置 ack 只证明该 revision 已加载，不证明真实外部 provider 可用。
- 默认 workspace/模型角色随配置重启及恢复后的数据发布保留。恢复发布还核对 installation，外来 installation 或陈旧 revision 不得改活动配置。
- 离线灾备只解析存储必需字段，认识但忽略运行态 model_routing；路由缺失/损坏/null 不阻断离线 inspect。未知存储字段仍拒绝，未开放任意 override。

## 2. 最终源码验证

| 检查 | 实际结果 |
|---|---|
| `pwsh -NoProfile -File tools/verify-code-quality.ps1` | PASS。包含 Rust fmt、workspace all-targets check/test、strict Clippy、前端安装/审计/TypeScript/行为、静态资源重建核对、bundle、原生打包输入、临时 NTFS 权限、完整默认合规和 diff 检查。 |
| 上述 workspace Rust tests | 161 suites，867 passed / 0 failed / 5 默认 ignored；不重复累计专项门。 |
| 前端行为与结构 | 54/54；592 个源文件，63 处既有规模债务未增长，未刷新 baseline。 |
| `tools/verify-product-configuration.ps1` | PASS。Host 配置/激活/维护 26/26、受管主脑正式 runtime 1/1，实际 Host/Web/core 综合进程与本机权限测试 2/2。 |
| `tools/verify-core-brain-workspace-contract.ps1` | PASS。18 crates、内部依赖边、97-kind strict prefix 和仅两个追加事件保持，99 个脑 EventKinds 不变。 |

最终统一门启动后未再改源码。源码门内原生 NTFS 7/7 仅证明临时目录权限行为，不证明服务账户 token 或已安装系统的访问拒绝。本轮未重新执行完整 M5 release audit 或真实 provider 验收；默认 ignored 不计通过。

本机日志位于 `target/installed-product-evidence/managed-preferences-20261024/`，保留且未随 build cache 删除；不把这些未签名日志当发布回执。

| 日志 | SHA-256 |
|---|---|
| `code-quality-final.log` | `EDF76AA8F72466DCE783F88E1FBE812F7A74185037628880246E10F918B12FCF` |
| `product-configuration.log` | `697426A31CE5263462EC08AADB6A457EC3B95A8285905E743441295FBA352D6A` |

## 3. 实际进程与浏览器检查

综合进程门使用正式 binary、自建临时安装根和 loopback 计数模型：保存 revision 2 的 workspace/两个模型角色 → 旧 core 保持原快照及 key → 停止并重启测试 core → acknowledgement 2 → 正式 Web run 绑定新 workspace → HTTP 请求和 ModelCall 事件均指向配置的前台 provider。后台 runtime 专项另外证明相关性对照和 cognitive tick 使用配置的后台 provider。缺失路由拒绝保存，恢复与只读查询不增加模型调用。

真实 IAB 浏览器登录临时 fixture，查看设置中的默认 workspace、前台/后台角色、有效来源及只写 key 输入；填写 `../escape` 后保存被浏览器格式校验拦截，刷新仍是原 revision/原配置。本次没有保存有效配置、改口令或操作真实 owner 数据；没有新的截图文件。该浏览器预览在检查后被显式中止，不作为综合进程完成证据；上表 PASS 来自另外完整执行并正常结束的专项门。

早期一次并发运行在恢复阶段出现 Unknown，没有把它记为成功，也未证明它由负载导致。隔离重跑明确发现离线存储投影遗漏新运行态字段，修复并补否定测试后专项门通过；这不追溯认定早期 Unknown 的唯一原因。

## 4. 保留的整体义务

能力/MCP/plugin/package、消息渠道、代理和 LAN 的受管配置、强隔离与正式执行 driver、会话 helper、签名升级回滚、原生服务和完整 IP01–IP24 联合验收仍开放。本组件不是整个设置页面或安装产品完成。

本轮无新增依赖、crate 边或脑 EventKind；未注册系统服务、未读取真实模型 key、未合入 main 或推送。后续实施继续按[整体交付清单](approved-design-completion-ledger.md)推进。
