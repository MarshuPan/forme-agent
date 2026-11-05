# 旧脑初始化的数据激活与运行确认分界

状态：**owner 已于 2026-11-05 批准协议衔接、文档回填和隔离实现；本提案不授权本机安装、源数据接管或服务操作。**

承接已批准的[首次认领旧脑与备份候选协议](installed-bootstrap-legacy-brain-claim.md)，只补齐其正式装配接缝；不新增内核、crate、依赖边、脑事件、角色或通用恢复入口。

## 1. 当前可复现的契约冲突

原协议 §4/§5 要求 `ClaimActivatedV2` 前完成配置/ack 核对，同时规定 `Activated` 前不得启动 Brain。首次初始化又是认领账号后才填写模型。现有 `ProductConfigStore` 的 runtime ack 是 `ProductConfigAcknowledgement`：要求非零配置 revision，由启动的 core 在真实数据/配置检查后产生。未配置模型的 revision 0 无法提供这种 ack。

因此不能通过提前启动 Brain、伪造 runtime ack、导入旧模型密钥或将 `Committed` 当 `Activated` 来拼接。这不是新的功能需求，而是原初始化顺序中的两种确认尚未明确分开。

此外，现有 `SignedWebClaimCommit` 只承载 schema 1 的新脑认领。它不绑定 schema 2 的 candidate digest、真实接管 receipt 或 activation。直接将其用于旧脑会丢失已冻结的关键绑定。

## 2. 拟定的两种确认

| 确认 | 时点与来源 | 能证明什么 |
|---|---|---|
| `BootstrapDataBindingConfirmationV2` | 同一 Host claim operation 的数据/config CAS 后，由固定离线 verifier 重读配置快照、新目标、owner/store/manifest 和恢复授权水位。 | 数据已完整接入当前安装的配置绑定；不证明模型或 ProductionRuntime 就绪。 |
| 既有 `ProductConfigAcknowledgement` | owner 后续保存有效模型配置，Host 按正常路径自动启动 Brain，core 真实装配后产生。 | 当前非零配置 revision 的唯一主脑已完成数据和运行配置确认。 |

`ClaimActivatedV2` 的初始化前置确认明确为第一种。它完成后可提交网页登录账号并进入“主脑待配置”；`BrainReady` 仍必须等待第二种。两者不能互相生成、替代或被 UI 合并成一个成功状态。

## 3. Additive 产品对象

下列仅为产品对象，不进入脑 EventKind。

`BootstrapDataBindingConfirmationV2`：

| 字段 | 约束 |
|---|---|
| `schema_version` | 固定 2。 |
| `install_ref, operation_id` | 必须绑定原 claim。 |
| `receipt_digest` | 原 `BootstrapClaimReceiptV2::digest()`。 |
| `expected_config_revision` | 首次接管固定为 0；已配置安装不接受。 |
| `config_binding_digest` | CAS 提交后的受保护 config-state 快照摘要，包含 owner 与实际 active-data binding。 |
| `target_binding_digest` | 与 receipt 和最终 activation 的目标摘要相同。 |
| `execution_fence_digest` | Store 在新副本上封存并经只读重验的恢复授权水位。 |
| `verification_digest` | 固定 verifier 对新目标实际解密、integrity、continuity、replay、manifest/owner/store/schema 的观察摘要。 |

`SignedBootstrapClaimActivationV2`：`schema_version:2, receipt:BootstrapClaimReceiptV2, activation:BootstrapClaimActivationV2, signature:Vec<u8>`。

签名消息为 `canonical_digest(("forme-bootstrap-claim-activation-v2", receipt, activation))`，使用现有受保护 bootstrap signer；Web 仅信任安装器预置的 bootstrap 公钥。先完整验证 receipt/activation 的既有方法及相互绑定，再验证签名，最后匹配 Web 认证库中原 operation 的账号承诺；不能从响应创建新的临时账号承诺。

原 receipt 是不可变历史事实。其 `expires_at` 约束首次 commit，不允许为重试修改 receipt 或 activation digest。已 `Activated` 后，可对同一原 setup operation/账号承诺重取相同签名结果，用于完成原 Web 账号事务；这不延长 setup 的发现、选择或数据写权限。未激活或绑定不符时不能签发本对象。

## 4. 正式事务顺序

1. 原 setup session 认证并只读验证候选；Host journal 记录选择；Web 在独立认证事务中预存账号 hash/承诺及 candidate binding，口令不出 Web。
2. 固定数据 worker 先持久化绑定完整原 operation 的 intent，再在新的空目标准备库、keys 和产物；复核成功才形成原 worker receipt 与 `ClaimDataPrepared`。未知结果只探测原 intent/receipt，不换 job、owner、候选或目标。
3. Host 核对原账号承诺、选择、prepared target/receipt，追加 `ClaimCommittedV2`。此时既不创建可用 owner session，也不启动 Brain。
4. 同一 Host 产品控制事务以 revision 0/未配置/未绑定其他 owner 为 CAS 条件，提交真实 owner 与 active-data binding。保持 `configured=false`；不创建虚假的 core 配置、模型、凭据或 runtime ack。已配置安装和另一 operation 的绑定都拒绝。
5. 固定离线 verifier 在 CAS 后重新读取并验证目标和配置快照，生成 `BootstrapDataBindingConfirmationV2`。原目标、keys、store、owner、schema、manifest、恢复授权水位或快照摘要不一致即 `RecoveryRequired`。
6. Host 独立持久化上述确认后，按原 receipt/目标 digest 追加已有 `ClaimActivatedV2`，再签发 `SignedBootstrapClaimActivationV2`。Web 只在签名、原承诺及 activation 全部匹配后原子创建 owner 账号并消耗 setup。
7. 网页进入模型配置。有效保存后走既有自动启动/真实 core ack/doctor 路径；此后才展示 `BrainReady`。不增加手动“启动主脑”的日常要求。

不得把普通 `CommitRestore` 或 `bind_owner` 的宽接口作为首次接管入口。config-store 增加 crate-private 的首次绑定操作，验证原 claim 的已知终态，保留正常 restore/已认领安装的全部既有拒绝规则。

## 5. 崩溃与恢复

源数据始终保留；数据写入前的 intent 与原 receipt 不清理。prepared、commit、config CAS、离线确认、activation 和 Web account commit 每一处都须注入中断。

重启不能自动继续复制、提交、切换、创建账号或启动 Brain。未有可信 activation 时保持初始化维护态；只能由原 operation 的只读查询核实阶段和现有回执。config CAS 已提交而 activation 尚未落盘时，仍为 `RecoveryRequired`，不能仅凭目标存在补写成功。显式恢复另验证原 intent、prepared/commit、当前 config、离线确认及所有摘要，不重做副作用；证据不足保留现场等待裁决。

已有可信 activation、但 Web 的响应或账号提交丢失时，只允许重取相同的签名 activation 及匹配原承诺的账号事务。不能以不同口令/用户名重新准备候选 hash，不能改绑 owner。

## 6. 回填与验收

确认后先回填 legacy-claim 提案 §3–6、prd/25 §3/§5.1、architecture/10 §3/§8 及必要的 canonical §29.5，再实现 additive 对象和正式装配。schema 1 新脑流程与既有 runtime ack 不改语义。

隔离验收须覆盖完整成功链及第 5 节每一中断点、错误签名、公钥替换、candidate/receipt/account/config TOCTOU、revision 0 CAS 并发、原号重取、跨 owner/已认领拒绝、零空脑替代、激活前零 Brain 启动、激活后仍未配置且无假 runtime ack、模型配置后真实自动启动与 ack。通过隔离验收不代表本机接管或 IP13/IP24 原生验收。
