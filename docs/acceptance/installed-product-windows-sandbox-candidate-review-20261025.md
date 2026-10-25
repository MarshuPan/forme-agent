# Windows worker sandbox 候选审查

日期：2026-10-25。范围：PRD24 §4.1.2 的 Windows 每 job AppContainer 隔离候选；本记录是静态源码与公开 API 审查，不是原生平台验收，也不改变生产 fail-closed 状态。

## 1. 审查结论

当前审查过的候选都不能直接接入 `forme-execution` 的生产 worker dispatch：

| 候选 | 可以复用的机制 | 阻断原因 | 结论 |
|---|---|---|---|
| 现有 `win-custody 0.1.1` | Job Object、挂起后先入 Job、受控进程/Token 启动、ACL 查询与整树 descriptor 应用 | 组件自身明确不是 sandbox；没有 AppContainer profile/capability、每 job token 或逐 mutation grant/revoke；整树重置不能代替保留原 ACL 的精确撤权 | 只能作为现有 Job/ACL 观测底座 |
| `landstrip 0.20.9` | Windows AppContainer、AppContainer capability、Job Object、受限用户路径、基础网络能力控制 | AppContainer identity 由工具、策略、时间、PID 和进程内计数器生成，不能由 job/plan 确定性恢复；ACL 由进程内 RAII `Drop` 撤销，不接入 forme 的 `sandbox-leases.jsonl`；库公开入口只有 CLI `execute()`，平台实现为 crate-private；AppContainer 路径未提供挂起后 token 核验接缝 | 不接入 |
| `rappct 0.13.3` | 可公开创建/派生/删除 AppContainer profile，构造 capability，查询 token，创建带资源限制的 Job Object | ACL API 只有 grant，没有逐 mutation revoke/readback；公开 launch helper 在创建进程后才附加 Job，不能满足首条指令前先入 Job；`suspended` 只留下未公开的恢复接缝 | 不接入 |
| `sandboxrs-windows 0.1.1` | 公开 builder、AppContainer backend、挂起创建、Job assignment 后 resume、管道隔离 | identity 自动追加时间/PID/计数器随机后缀；递归 ACL 只 grant，没有 durable lease/revoke/readback；profile 的生命周期与 forme job journal 无绑定；后台实现依赖 `rappct`，不能把缺失的回收契约补出来 | 不接入 |
| `skarn-sandbox 1.0.1` | Windows AppContainer 与 Job Object 的统一高层 API，默认拒绝网络 | 使用固定共享 profile；正常创建进程后才附加 Job，存在首条指令前的 custody 缺口；ACL 只 grant，未提供逐 job revoke/readback 或 durable lease；不能表达 forme 的 peer/token/plan binding | 不接入 |

这些候选提供了实现参考，但不构成授权、依赖批准或安全证据。没有候选可以把“有 AppContainer/Job Object”提升为 IP10/IP24 通过。

现有 `win-custody` 的公开 ACL 读取能力可以用于未来 adapter 的有效权限回读辅助，但其整树 descriptor 应用会重置目标 DACL，不能用来掩盖或替代 `SandboxLeaseJournal` 要求的逐 mutation、保留其他 owner ACE 的精确撤权。

## 2. 与 PRD24 §4.1.2 的逐项对照

| 冻结门 | 当前候选状态 | 必须由 forme adapter 负责的闭合 |
|---|---|---|
| identity | 候选可创建 profile，但 identity 不满足 `runtime_root + job/plan binding` 的稳定恢复约束 | 使用规范化、域分隔的固定摘要派生 profile name；持久记录 name 与 SID；启动时按原 job 重新派生并核对 |
| 首条指令前隔离 | `landstrip`/`rappct` 公共入口不满足；`sandboxrs` 的内部实现满足 Job 先于 resume，但不暴露 forme 需要的审计对象 | 挂起创建，先安装 AppContainer/低完整性 token 与 kill-on-close Job，核对有效 token、句柄继承和 profile SID，再 resume |
| ACL intent | 候选没有写入 forme 固定 journal 的接缝 | 每个 grant/revoke 前先调用 `SandboxLeaseJournal::begin_acl_*`，绑定 job、plan、identity、规范化路径和 file identity |
| ACL readback/revoke | 候选没有符合要求的持久逐步回收；`Drop` 失败也不能形成可恢复事实 | 执行后回读有效 ACL；终止进程后按 mutation 精确撤权；每一步追加 receipt；失败阻断新 admission |
| 网络/凭据 | `landstrip` 可表达更宽策略，其他候选默认无网络但没有 forme grant binding | 当前 Worker DTO 没有 network/credential grant；adapter 必须拒绝继承网络、代理、secret 和 loopback exemption |
| 崩溃恢复 | 候选各自拥有进程内或独立 JSON 状态，不是 forme 单 writer append-only lease | Host 重启扫描 worker journal 和 sandbox lease；未知、身份冲突、file identity 变化或回收失败均保持 blocked |
| 正反证据 | 候选自身测试不能替代 forme 的安装身份、有效 token 和文件访问证据 | 用真实 worker 读取/写入精确 workspace、brain/key/兄弟 workspace、网络和凭据的正反断言；分别核对 service peer 身份 |

## 3. 保持的实现边界

- `SandboxLeaseJournal` 继续只负责 durable intent、readback、恢复投影和 admission fence；它不假装执行 ACL，也不接受候选库的隐式 `Drop` 作为 cleanup receipt。
- `WindowsJobWorkerIsolation` 继续只声明 Job Object 进程树 custody。它不升级为完整 sandbox，不绕过 `UnavailableWorkerIsolation` 的生产选择条件。
- 不新增候选依赖，不把第三方源码、prompt、fixture 或目录结构复制到仓库；候选仅作为机制研究材料。
- 没有真实 LocalSystem broker、已安装 core peer、worker token、NTFS ACL、网络/credential 拒绝及崩溃后恢复证据前，Windows managed-service dispatch 仍拒绝；IP10/IP24 保持未通过。

## 4. 下一实现接缝

下一次实现应先补一个 forme-owned Windows adapter contract，而不是把候选的高层 `Sandbox` 对象直接塞进现有 `WorkerIsolation`：

1. `prepare(job, permit, execution)` 返回包含确定性 profile identity、SID/token proof、Job handle custody 和 lease id 的受限 launch state。
2. `grant`、`readback`、`revoke`、`remove_identity` 都必须与 `SandboxLeaseJournal` 的 intent/receipt 顺序绑定；任何中断进入原 job `Unknown` 或恢复阻断。
3. `launch` 只能在所有 ACL readback、token/identity 核对和 Job Object 设置完成后恢复主线程；失败时终止并回收，不得降级为普通 process。
4. adapter 必须提供真实 Windows 合同测试所需的 ground truth：worker token、AppContainer SID/完整性、句柄继承、精确文件访问、拒绝 brain/key/网络/凭据，以及重启后 lease recovery。

在该接缝和真实管理员服务身份完成前，继续使用现有开发前台开关验证 wire/receipt/Unknown 语义即可，不能把普通 worker 或单项平台机制登记为产品完成。
