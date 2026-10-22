# 离线数据维护与恢复候选组件验收

2026-10-23，施工分支 `shu/installed-product`，基底 `f9c7de8`。本批完成受管密钥库、在线/离线互斥、本机备份/验证/新路径恢复及正式回答的重启读取；它不切换活动数据，不代替 Host 完整恢复事务或 IP01–IP24。

## 实现与安全边界

- ProductionRuntime 在初始化数据库前取得 OS shared fence，由 authority guard 的所有 clone 持有；离线检查/恢复取得 exclusive fence。争锁失败不能读写脑或密钥，最后一个 Harness/services/authority clone 释放前仍阻断维护。
- 受管密钥构造使用安装器 ACL，不依赖交互 USERNAME 或临时 icacls。只读 existing resolver 不创建目录、不分配/销毁密钥、不改权限；缺失、破损、置换、链接与已撤钥均拒绝。
- `forme-hostctl` 沿 Gateway → Harness → Store 的正式外观运行 inspect/backup/verify/plan/restore；execution 未增加 store 依赖。配置只投影安装、owner/workspace 与存储绑定；坏模型/scheduler/transport 配置不会阻止灾难恢复，也不会启动运行时或读取模型密钥。
- 源库通过 cold-sidecar、schema、owner/store identity、integrity、事件连续性、内容列保护/行绑定、key history、真实解密和投影 replay 校验。只读检查不使用会恢复或初始化原库的普通 writable initializer。
- 备份与在线 BackupService 的 manifest 相互兼容，数据库 hash 流式计算，key 不进备份。恢复计划绑定源/manifest/owner/store/确切目标/15 分钟期限，在创建前再次检查。目标和计划/回执只 create-new，拒绝覆盖或删除重试；失败保留现场。
- 新路径恢复继续使用原 vault；缺 key 不能生成替代 vault。回执明确 `authority_switched=false`。成功创建恢复候选不表示活动数据切换、旧审批/外部动作 reconciliation 或原生服务恢复已完成。
- 正式 Gateway 在进程重启后从受保护事件恢复已提交公开回答。必须匹配 finished/classified model call 和 `final_output` RunComplete；未完成、tool/handoff、失败、后续内部思考均不能冒充回答。读取不新建 run、不调用模型、不补写事件。

## 本轮验证

| 门 | 证据 |
|---|---|
| 源码质量 | fmt、workspace all-target check、strict Clippy、默认完整 compliance、diff whitespace 均通过；结构门 501 文件、62 处历史规模债务，未刷新基线。 |
| 存储/密钥/旧迁移 | keys/access 5、offline recovery 5、legacy migration 10、旧进程迁移 7 场景通过；包含实际跨进程 OS 争锁、原件/keys 不变、manifest 互通、错误 owner/key、撤钥/血缘损坏、过期/置换/已有目标拒绝。 |
| 生命周期和重启 | authority clone 生命周期 1、原 vault 恢复运行时 2、流式摘要 1、公开回答恢复单元 2 场景通过。 |
| 正式进程 | 实际 host/web/core/hostctl 临时进程完成在线备份→离线验证→新备份→计划→一次恢复→新路径 core 启动→原回答恢复；恢复时 model 计数不增加。错误摘要零目标/回执，重复操作不覆盖，源无 WAL/SHM/journal，原 key hash 不变。坏运行配置允许存储投影检查，未知存储字段仍拒绝。 |
| 产品配置 | 原 21 个 Rust 配置/activation/maintenance 场景通过。首次并行负载下 bootstrap-proof 返回 Unavailable；独立复跑完整实际进程链路通过，未加重试隐藏写入，也未放松断言。 |
| 依赖审计 | 独立 `npm audit --prefix frontend --audit-level=moderate` 最后成功，0 vulnerabilities；此前 registry ECONNRESET 不是漏洞通过证据。 |
| 完整 Rust 回归 | 当前源码 `cargo test --locked --workspace --all-targets` 退出 0；155 个 suite，802 passed / 0 failed / 5 默认 ignored。默认忽略项未被计作执行通过。 |
| D 运行时回归 | `verify-core-brain-closure.ps1 -Wave D -RuntimeOnly` 通过 C19–C23、真实迁移进程与 C24 runner fixture；不是 D/Final、真实 provider 或新的发布回执。 |

源码日志留于本地 `target/offline-recovery-*`。永久 M5 全门另跑并分别记录运行时与发布回执结果；历史失败日志不冒充通过记录。

## 已修复的回归

第一次全量发现 C6 的 durable-store 拒绝诊断发生漂移，恢复明确 durable 诊断后专项及全量通过。第二次发现旧迁移的 plan 创建 access-lock，破坏零文件创建；已改为使用已有 fence，原库 handle/advisory lock 覆盖首次建 fence 的竞态，7 个真实迁移进程场景通过。未删减或改松原断言。

## 仍需继续完成

Host PrepareRestore/CommitRestore 的候选登记、受认证活动数据/config CAS、旧 authority/审批/Unknown fencing、旧脑接管、完整 Upgrade/Rollback 与自更新 handoff 继续实施。Windows 组件证据不替代管理员安装、真实服务账户 token/ACL、强隔离 worker 或 Linux/macOS 原生运行。本批未读取 `.forme` 私有数据/keys、未注册本机生产服务、未推送或合 main。
