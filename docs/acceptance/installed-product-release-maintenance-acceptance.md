# 签名发布暂存与窄维护入口组件验收

2026-10-23，施工分支 `shu/installed-product`，基底 `35298b3`。本记录只证明本批组件；IP14/IP15/IP16/IP21/IP24 和全产品里程碑尚未关闭。

## 实现

- 离线 Ed25519 签名工具验证 bundle inventory、封闭文件集、每个大小/摘要与显式兼容范围，再输出 manifest、签名、content 和安全 receipt。Node 生成物由实际 Rust `forme-hostd --verify-release` 验签；独立管理员公钥配置是信任根，包不能自授信。
- Host 的 release worker 在 plan/execute 双重验证，先持久 Checking intent，再创建 operation 专属同卷暂存、同步、重验、原子发布和不可变 receipt。既有目标/中断目录保留，不覆盖重跑。UninstallPlan 只记录三项服务、程序目录和九类保留数据域，不执行卸载。
- Web 固定维护外观接 ProductJournal；认证 session、Origin/CSRF、稳定 operation、单次密码重认证和服务端 owner/digest 重建。只允许主脑启停/重启、签名暂存和保留数据的卸载范围计划。没有 maintenance bearer、任意 shell、SQL 或泛化管理 proxy。
- Host 保留 `OwnerReauthenticated` actor，复用 CAS/intent/receipt，不伪造管理员。产品 revision 与配置 revision 分开；start/restart 持配置锁校验至原生 IO，响应丢失后只读查询原 operation。unknown/recovery 不重派。
- 原生服务回执必须同时绑定 schema/service/action/terminal；restart 的 stop 不可核实时绝不 start。新增职责模块，未新增 crate、内部依赖边或脑 EventKind。

## 证据

| 检查 | 结果与边界 |
|---|---|
| fmt / workspace all-target check / test / strict Clippy | PASS；150 suites，786 passed、0 failed、5 默认 ignored。 |
| `py -3 tools/code-quality.py` | PASS；484 文件、63 处历史规模债务，不增长。 |
| release staging | 8/8：篡改、并发、崩溃暂存、错误绑定、旧目录保留和卸载范围。 |
| product config/activation/maintenance | 21/21，新增 5 项覆盖 owner/domain/CAS/期限/严格输入、并发重启后一次执行、unknown 不重派和真实 actor。 |
| 实际 host/web/core 进程 | PASS；初始化认领→模型配置→重启恢复→ProductionRuntime ack/Run；维护错误密码/CSRF/多余字段/陈旧产品 revision 不写 journal、不执行 SCM。模型是本地计数 fixture。 |
| 前端/内嵌资源 | 19/19；types、可重复构建、manifest 和资源逐字一致。 |
| bundle / 签名 | 9/9、4/4；签名包含当前实际 Rust verifier 的接受与内容替换拒绝，没有跳过。 |
| Windows 权限组件 | 7/7 临时 NTFS 场景；不是实际服务 token/管理员安装验收。 |
| 合规、diff | 默认参数 compliance doctor PASS；diff whitespace PASS。 |
| npm audit | `npm ci` 返回 0 vulnerabilities；独立 audit 连续两次因 registry TLS 连接失败，未取得新的独立审计成功，不标整个统一质量脚本 PASS。 |

完整源码脚本停在网络审计，随后单独完成前端、bundle、签名、权限和默认合规门；未修改源码来规避失败。前一轮资源 manifest 陈旧已通过正常生成器重建；早期并发/严格解析测试发现的问题已修复后重验。

## 继续义务

本批暂存不切换服务 image；完整 Upgrade/兼容 Rollback、Host/Web 自更新 handoff、新路径恢复与旧脑接管、受管密钥/离线独占、强隔离 worker 和原生包/平台证据仍需完成。网页范围计划不是卸载执行；源码测试不是正式安装或真实 provider 验收。没有读取 `.forme` 私有数据/keys、注册生产服务、推送或合 main。
