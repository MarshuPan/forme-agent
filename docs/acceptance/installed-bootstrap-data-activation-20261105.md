# 旧脑首次认领：离线数据激活与账号衔接（2026-11-05）

本批承接 owner 已批准的[激活分界方案](../architecture/proposals/installed-bootstrap-data-activation-composition.md)，覆盖正式源码与隔离验收。文档契约已先以 `d8fdeec`、`968f571` 提交，实现提交为 `67a6bf6`，分支 `shu/bootstrap-claim-journal`；本报告不授权本机旧脑接管、安装程序替换、服务启停或原升级重试。本批没有合入 main 或推送。

## 1. 实现范围

完整路径为 Web 的候选选择/原账号承诺 → 固定 Host setup API → Host journal 与配置锁 → `forme-hostctl maintenance-bootstrap-data` → Gateway 离线 adapter → Harness → Store backup/restore → revision 0 的 owner/data CAS → 离线重验确认 → 持久 activation 与签名 → 独立 Web 认证事务。执行模块不直接打开脑数据库，产品事实不进入脑 EventKind。

- 真实候选发现只使用安装器派生的历史库/备份根；以只读解密、owner/store/schema、完整性、重放、水位及产物摘要形成 catalog，由 journal 单写者分配 epoch。错误 key/owner、未知 schema、损坏、链接/别名、占用或缺少可认证内容均不变成空脑候选。
- Host 派发前写入原 operation 的 `ClaimDataIntentV2`；固定子进程在复制前持久化自己的 create-new intent。复制到新的固定目标，保留源库、keys 和产物，不合并或清理既有目标。
- Store 在副本上建立恢复授权水位；Host 只在绑定原选择、账号承诺、prepared receipt 后提交。配置首次绑定限定 revision 0、未配置/未绑定安装；不能借普通 restore 或宽 owner 接口换绑。
- 数据与配置真实重读后形成 `BootstrapDataBindingConfirmationV2`。`ClaimCommittedV2` 不等于 activation；只有确认落盘后才追加 `ClaimActivatedV2`，再签发 `SignedBootstrapClaimActivationV2`。
- Web auth schema 10 保留原 setup/candidate/账号 hash 承诺，只接受安装器预置公钥签发且完全匹配的 activation。口令不发送给 Host；无 activation 时不生成可用 owner 账号或 session。
- Host 重启后未激活的 intent/prepared/commit/config-CAS 状态保持 `RecoveryRequired`，不再复制、签发或启动 Brain。已有 activation 的原号重取不修改 receipt 到期时间，也不能替换账号或候选。
- 选择响应丢失时，原 setup 只读取得 Selected 阶段的原 catalog/state；ref/digest/epoch 全部相等后才能补记原选择。该路径不调用 select/adopt，不重开 prepared/committed/待恢复事务。
- schema 1 新脑流程保留。只有既未声明历史源根、也未挂载候选 worker 的旧配置可沿用检查空目标的兼容观察；worker 失败绝不降级到这个分支。

离线数据绑定后仍为 `configured=false`、revision 0，无 core 配置、模型凭据、服务启动或伪 runtime ack。后续保存模型继续使用既有自动激活与真实 `ProductConfigAcknowledgement`；该 ack 与本批离线确认是两类事实。

## 2. 验证层次

| 层次 | 实际覆盖 | 证据边界 |
|---|---|---|
| 真实临时 Host/固定 worker | 加密旧库与备份恢复、源与产物保留、并发同义请求只执行一次、Host 重启原号查询、config CAS 后 activation 前中断、worker EOF 与快照/receipt 替换拒绝。 | 真正启动仓库程序及 Store 操作；使用隔离目录与原创数据，不是已安装服务或用户旧脑。 |
| 真实临时 Web HTTP/认证库 | setup cookie、CSRF、schema/闭合输入、原选择找回、错误摘要/签名、账号承诺与 activation 后 owner session。 | Web/认证事务真实；Host 响应与签名权威为受控 fixture，不是整机接管。 |
| journal/config 组件 | 每个持久事实中断、不可变 expiry、不同账号/确认/目标拒绝、revision 0 并发 CAS、无 ack/no-core、旧 schema 兼容。 | 验证状态机与持久绑定；不替代实际源发现和程序管道。 |
| 实际 Store 防反例 | crypto-deleted envelope 不计为可认证 owner 内容；未知保护/无有效内容不伪装为已验证旧脑。 | 在隔离库和 keys 上执行，不扫描本机私人脑库。 |
| 既有模型配置/核心回归 | 当前源码的模型配置、自动激活、真实临时 core ack、S1–S99 与相关 C 门。 | provider/服务夹具与原生/live 证据分开；不能从现有新脑流程测试推定旧脑安装全链路通过。 |

## 3. 最终门与原始证据

原始日志保存在本工作树 `target/installed-bootstrap-activation-evidence/`，最终门在其 `complete/` 子目录收取；初轮 Clippy 类型复杂度拒绝、测试读取活 journal 的 Windows 锁拒绝、固定 10 毫秒测试时钟早于真实 catalog 观察的拒绝均保留。修正使用具名类型、fixture 退出后的字节核对及从持久观察推进的测试时钟，没有放宽生产锁、Clippy 或时钟回退拒绝规则。

| 最终门 | 结果 |
|---|---|
| `tools/verify-code-quality.ps1` | PASS，exit 0；182 Rust targets、1230 passed / 0 failed / 15 默认 ignored，fmt/check/strict Clippy、854 文件规模门（63 项既有债务未增长）、前端 110/110、锁定依赖与可重现构建、打包/签名/临时 NTFS/只读防火墙策略、完整默认合规通过。 |
| `tools/verify-installed-host.ps1` | PASS，exit 0；326/326 个逐名场景，每个恰好实际运行一次，含本批真实固定进程场景。该门明确不提升 IP01–IP24。 |
| `tools/verify-web-identity.ps1` | PASS，exit 0；27/27 命名身份场景。 |
| `tools/verify-product-configuration.ps1` | PASS，exit 0；82 项配置/激活/维护组件、网络 fence 与受管偏好/skills，以及 4 项真实临时 Web/Host/Core 多进程用例通过。真实安装/SCM/provider 仍不在该门范围。 |
| closure A/B/C | PASS，exit 0；分别 47/47、50/50、36/36 个逐名场景；运行回执绑定实施提交 `67a6bf6`。 |
| closure D `-RuntimeOnly` | PASS，exit 0；71/71 个逐名场景，运行回执绑定实施提交 `67a6bf6`；不代表 D/Final 或新的 C24 live。 |
| M5 / S1–S99 与 current-tree 发布回执 | 单独在干净提交上执行，最终状态与不可省略的回执比较见 [M5 报告 §14](m5-acceptance-report.md#14-旧脑离线激活源码后的重验2026-11-05)。 |

定向回归另外验证真实 Host/固定 worker **8/8**、Web HTTP **4/4**、execution 库 **228 passed / 0 failed / 1 默认 ignored**；这些不是新增数量，也不与全工作区重复相加。离线候选六项、身份 27 项和配置 82 项由永久门收取，不以未执行的列表当作通过。

跨 crate 多进程回归前先构建当前 workspace 程序，防止使用旧缓存程序；永久宿主门逐名执行新增场景，配置门与身份门继续校验确切执行数量。前端嵌入资源由锁定依赖重建，不手改生成产物。

上述八个已完成门的原始日志 SHA-256 如下，均位于 `complete/`；运行摘要 `verification-summary.json` 继续由同一驱动追加 M5 结果，不覆盖早期失败日志。M5 最终日志/回执及整轮资源报告统一登记在前述 M5 §14，避免以尚未完成的最终结果污染本批事实。

| 日志 | SHA-256 |
|---|---|
| `code-quality.log` | `a971b0d8970d0b3837aee02c1f47501511c77e7a9a94f4b1db37fb3916f63dc5` |
| `installed-host.log` | `edc6f00ecf26e321df371634f52c540730ba5fd26ffa1f0e3b7f897557187cb5` |
| `web-identity.log` | `f948f53d3311377626828a3f415e471d3df3b1b8d2da3d2d4696cdd6f405dea3` |
| `product-configuration.log` | `dfa3384644d5abeecb7b88aeb9af82a08eb5d460ad85ca5c780ed4d672ffe51b` |
| `closure-A.log` | `2b7a5e78491026af5c94fa1f4c41b89dd3e4be6348e482a26d8a509561a02671` |
| `closure-B.log` | `54add7bcd51827f0988232a7b581b44480cb774a3b99ac1f698c25466208ff1c` |
| `closure-C.log` | `d5a55923fd121502e3950f4292f84e3b03f0298913f8934d3a47051a3473cb27` |
| `closure-D-runtime-only.log` | `304327d38afb07a9f7ca7dc3fb2ec24cc38865c255d0b5f1a379c32f904d655b` |

## 4. 尚未闭合的原生与恢复范围

1. 本机未接管历史数据、替换管理程序、迁移现场 journal 或启停服务；完整浏览器/SCM/真实 key ACL/独占锁与首次接管 IP13/IP24 仍未验收。
2. 新协议明确拒绝未激活的未知结果。通用显式继续/裁决入口没有实现或借本批扩大；需要原 intent、prepared/commit、配置和独立确认的精确证据与授权，不能自动补写成功。
3. 接管完成后保存模型并在真实安装中自动启动、取得非零 runtime ack、执行真实 provider 对话仍需联合验收。组件门和运行时回归不替代该证据。
4. 本机缺基线的旧 Upgrade 是独立未决操作。原生历史 collector、档案本源/连续性、实际独占数据核查和签名管理 reader 修复仍开放；本批不能绕过它，也不重发原升级。

整体目标保持开放。Linux/macOS 继续遵守 owner 已选择的有限恢复与不发布边界，没有用跨编译或临时进程宣称原生产品完成。
