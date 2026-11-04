# 首次认领旧脑与备份候选协议

状态：**已获 owner 批准，先完成文档冻结，再实现协议与隔离测试；不授权本机数据接管、迁移、切换或服务操作。**

本提案补齐产品化首次初始化中“新脑之外的已有脑/备份候选”这一承重接缝。它不是普通 `CommitRestore` 的别名，也不是已认领安装上的恢复入口；它只适用于**尚未认领的安装实例**，并且必须在安装器/Host 派生的固定路径上完成只读发现、验证、显式选择和一次性认领。

## 1. 目标与不变边界

首次初始化的三种候选类型固定为：

| 类型 | 语义 | 首次认领处理 |
|---|---|---|
| `NewBrainTarget` | 安装器创建的空、受保护目标目录；没有历史 OwnerId。 | 由 claim 事务生成唯一 OwnerId，并在目标上初始化新的脑绑定。 |
| `ExistingBrain` | 安装器固定布局中发现的历史 forme 脑/数据源；已有 OwnerId、store identity 和 schema。 | 先只读验证，再复制/迁移到新的候选目标并在认领事务中绑定已验证 OwnerId。 |
| `VerifiedBackup` | 安装器固定备份目录中发现的、具有完整 manifest/key 可用性和 continuity 的候选。 | 先只读验证，再恢复到新的候选目标并在认领事务中绑定已验证 OwnerId。 |

检测到历史明文、错误 key、错误 owner、未知 schema、损坏、链接/别名、占用或无法建立独占观察时，候选只能是 `Rejected` 或 `Unverifiable`，不可被浏览器选择；系统不能为了继续初始化而创建另一个空脑。

本协议不改变：18-crate 图、99 个脑 `EventKind`、既有 `CommitRestore`、已认领安装的 owner/数据边界、脑 EventStore 单写者或 Agent 执行治理。产品管理事实仍在独立 claim journal/认证库/Host 数据事务中，不进入脑表、不推进 run。

## 2. 候选发现与证据

### 2.1 路径与信任来源

候选发现只能由安装器预置的 `InstallationManifest`、`BootstrapAuthorityConfig` 和 Host 固定布局派生。浏览器、模型、Web setup session 和普通维护请求不得提交路径、URL、SQL、目录、owner、key、程序或“已验证”布尔值。

Host 只在未认领 installation 上运行候选发现，并先取得候选源的独占访问证明：

1. 核对安装身份、manifest/config schema、固定根目录和无链接/别名/路径交叠。
2. 核对现有脑/备份源没有活动 writer、SQLite WAL/SHM/journal 或未决 worker；不能用“服务当前显示停止”替代独占锁。
3. 以只读方式核对 store identity、OwnerId 来源、受支持 schema、key resolver 可用性、integrity、event continuity、projection replay 和保护状态。
4. 对备份核对 manifest/checksum、owner/store/schema、key history 和恢复目标约束；不把备份文件名或用户提供的摘要当作证据。
5. 对历史明文只记录 `NeedsMigration`/`Rejected`，不在本协议内自动加密、覆盖原件或生成新 OwnerId。

### 2.2 候选安全视图

Host 返回 `BootstrapCandidateCatalogV2`，只含 opaque `candidate_ref`、候选类型、验证状态、候选 digest、schema/protection 状态、Host 验证产生的 `owner_ref`/owner binding 和 issue ref；不含绝对路径、原文、key、私钥、cookie、凭据或可推导路径的文件名。`owner_ref` 是安全的稳定引用，不是浏览器提供的身份，也不是凭据；历史候选不能只带 owner digest 而省略该引用。

候选 digest 是 Host 按固定 canonical tuple 重新计算的值：

```text
(schema_version, install_ref, candidate_ref, candidate_kind,
 source_observation_digest, owner_ref, owner_binding_digest, store_identity_digest,
 schema_digest, protection_state, verification_epoch)
```

`source_observation_digest` 覆盖本次只读检查到的源摘要、manifest/checksum、锁和 replay 结果；`verification_epoch` 防止把旧观察当成当前事实。浏览器提交的 candidate digest 只作为 expected value，权威值始终由 Host 重新计算。

Owner 来源必须显式标记为 `GeneratedForNewBrain` 或 `VerifiedFromCandidate`。对于已有脑/备份，`VerifiedFromCandidate` 只能来自成功解密、完整性与 store 元数据核对，不能来自浏览器输入、登录名或候选标签。

## 3. Setup session 与认领绑定

现有 schema 1 DTO/客户端继续兼容 `NewBrainTarget` 的旧路径；旧路径不能看到或选择已有脑/备份。候选接管使用 schema 2 的 additive DTO，不修改脑协议：

- `BootstrapCandidateCatalogV2`：候选列表和当前 verification epoch。
- `BootstrapCandidateSelectionV2`：`session, candidate_ref, expected_candidate_digest`。
- `BootstrapClaimRequestV2`：`session, candidate_ref, expected_candidate_digest, account_commitment`。
- `BootstrapClaimCommitV2`：claim receipt、其中的 owner/candidate binding 和迁移结果的安全摘要；不包含 active data binding。

setup session 持久化 `candidate_ref, candidate_kind, candidate_digest, verification_epoch, owner_source, owner_ref, owner_binding_digest`。选择后必须重新取得同一候选的只读验证；候选 digest、owner/store/schema、安装身份或锁状态有任何变化，返回 conflict/recovery required，零账号、数据或 active binding 写入。

浏览器不能提交 OwnerId。新脑由 broker 生成 owner；已有脑/备份由验证结果提供 owner。claim 请求中的 expected digest 不是授权，也不能替换 Host 观察；它只用于发现页面与提交之间的 TOCTOU 漂移。

## 4. Claim journal 与状态机

产品 claim journal 从 v1 进入 v2 时保留 v1 字节和 hash，不回写历史记录。v2 reader 可以读取完整 v1 前缀后追加 v2 facts；写入首个 v2 fact 后，旧 reader/writer 必须拒绝并保持 fail closed。不能用内存状态代替 journal。

本次新增的产品事实为：

| Fact | 作用 |
|---|---|
| `CandidateInventoryObserved` | 记录受保护候选集合 digest、verification epoch 和安全状态摘要。 |
| `CandidateSelectedV2` | 绑定 setup operation、candidate ref/kind/digest、Host 验证的 owner source/ref 和 owner binding digest。 |
| `ClaimDataPrepared` | 绑定新目标、源观察、目标摘要和迁移/恢复 worker 的已知终态；它是 Host 构造的受信结果，不表示已激活。 |
| `ClaimCommittedV2` | 在账号承诺、候选绑定、OwnerId、目标数据和 claim operation 全部一致后提交一次性认领事实。 |
| `ClaimActivatedV2` | Host 已按同一 operation 完成 active data/config 切换并核对新目标；只在可验证终态追加。 |
| `ClaimAbortedV2` | 仅用于 owner/本机管理身份明确终止尚未发生数据副作用的 pending claim；保留全部历史。 |

状态只允许沿以下顺序前进：

```text
Observed -> Validated -> Selected -> DataPrepared -> Committed -> Activated
             |             |            |             |
          Rejected      Conflict    RecoveryRequired  Aborted
```

`Committed` 不是“数据已切换”；在 `Activated` 前 Web 仍显示初始化维护态，不能启动 Brain。`ClaimActivatedV2` 还必须绑定已提交 claim receipt digest 与 active data binding digest。`DataPrepared` 后任何写入结果不明、目标摘要缺失、worker 失联或服务重启都保持 `RecoveryRequired`，只允许原 operation 的只读核实/owner 裁决，禁止换候选、换 OwnerId、重发复制或盲目删除目标。

状态视图的字段矩阵也是协议的一部分：

| phase | candidate ref/digest/epoch | receipt | activation + active binding | issue ref |
|---|---|---|---|---|
| `Observed` | 全部为空 | 空 | 成对为空 | 空 |
| `Selected` / `DataPrepared` | 全部必有 | 空 | 成对为空 | 空 |
| `Committed` | 全部必有 | 必有且绑定 candidate/operation | 成对为空 | 空 |
| `Activated` | 全部必有 | 必有且绑定 candidate/operation | 成对必有且绑定 receipt | 空 |
| `Aborted` | 全部必有 | 空 | 成对为空 | 必有 |
| `RecoveryRequired` | 全部必有 | 可有（若已有可信 commit） | 成对为空 | 必有 |

部分 candidate binding、部分 activation binding、带有互相矛盾的 receipt 或在终态混入 issue 都是无效状态；`RecoveryRequired` 不代表数据已激活。

账号承诺先以受保护的临时记录保存，明文密码不进入 journal；claim commit 必须绑定真实 owner、候选 digest、account commitment、operation、install ref 和 expected revision。响应丢失或进程重启只按原 operation 重取相同 receipt；异义重试冲突且零推进。

## 5. 数据接管与恢复语义

- `ExistingBrain` 与 `VerifiedBackup` 均恢复到新的、空的、安装派生目标；不在原源上原地改写，不覆盖 active brain，不删除原件。
- 数据 worker 先建立受保护 intent，再执行有界复制/恢复，完成后重新计算目标 manifest、store identity、owner、schema、key/projection/replay 摘要；worker 自报或退出码不能单独形成 `ClaimDataPrepared`。
- 目标已经存在、源摘要漂移、源重新被占用、错误 key/owner、明文保护状态、磁盘空间或 schema 不兼容，均为拒绝或恢复状态，不降级为 `NewBrainTarget`。
- `ClaimActivatedV2` 需要同一 Host product transaction 完成 active data/config CAS、旧 authority fence 和配置/ack 核对，并绑定已提交 claim receipt digest 与 active data binding digest。只读验证、`ClaimDataPrepared` 或 `ClaimCommittedV2` 不能声称已完成接管。
- 首次 claim 完成后，普通 bootstrap 端点失效；已认领 installation 不能用另一候选暗换 owner。后续接管必须走显式、独立的 owner-approved restore/re-adoption 流程。

## 6. 失败、未知与重启恢复

| 情况 | 结果 | 允许的下一步 |
|---|---|---|
| 未发现候选或只有空新脑 | 显示 `NewBrainTarget` 或 `DataUnavailable`。 | owner 显式选择新脑或修复候选。 |
| 错 owner/key、明文、损坏、未知 schema | `Rejected`，不写 claim。 | 仅查看 issue ref，不能强制选择。 |
| 候选正在使用/锁不明 | `Unverifiable`/`Conflict`，不写 claim。 | 停止旧 writer 后重新发起新的 verification epoch。 |
| 选择后 digest/epoch 漂移 | `Conflict`，保留原选择历史。 | 新 setup operation/重新验证；不沿旧选择继续。 |
| 数据准备前断线 | 原 operation 可查询，未提交动作不重做。 | 原 session 或本机管理身份核实/继续。 |
| 数据准备后副作用不明 | `RecoveryRequired`。 | 只读 probe、取原 receipt 或 owner 裁决；不换 job 重派。 |
| claim commit 响应丢失 | 原 operation 的 `Committed`/`Activated` 可重取。 | 同义重取，不提交新账号/新 owner。 |
| 已认领后再次访问 bootstrap | `Conflict`/`Unauthenticated`。 | 使用正常登录或独立恢复流程。 |

恢复扫描不能从浏览器临时状态恢复 setup，也不能因进程重启自动选择候选、创建账号、启动 Brain 或切换 active data。没有可信终态时保留源、目标、intent、receipt 和 journal 证据，清理必须等 owner 裁决。

## 7. 兼容、实现与验收边界

实现保持现有 `forme-protocol`、`forme-execution`、`forme-gateway` 的依赖方向，不新增 crate、EventKind 或脑 writer。Host/worker 只通过现有产品管理/离线数据边界工作；Web 只看到安全 DTO。

隔离测试必须覆盖：新脑旧路径兼容、三类候选发现、错误 owner/key、历史明文、损坏 manifest/schema、数据库占用、candidate digest TOCTOU、并发 setup、断电于每个 journal 阶段、响应丢失后的原号恢复、旧 reader 对 v2 拒绝、已认领拒绝和“零空脑替代”。测试 fixture 自建，不包含真实账号、key、用户正文或私人路径。

本提案实现完成只表示协议、DTO、journal 和隔离测试闭合；不表示当前本机旧安装已被接管，不授权迁移本机旧 journal/脑数据、替换程序、启停服务或把 IP13/IP24 标为通过。真实 IP13 仍须从正式安装包、真实权限、真实锁、真实 key resolver、原生服务和浏览器联合验收。
