# prd/25 初始化、认证、配置与 Web 接入

## 0. 定位与承上

实现 [requirements/17](../requirements/17-installed-product-scope-decisions.md) P03–P08、P10、P15，主验 IP04–IP09、IP13、IP21–IP22，参与 IP12/IP18/IP24。架构以 [architecture/10](../architecture/10-installed-product-and-web-workbench-architecture.md) §3–6、§8 为准，canonical §10/15/24/28/29 不可回退。

**状态：正在实施；`shu/installed-product@12b3f5d` 已提交独立账号/session 基础，尚未合入 main 或取得 IP 整体验收。** broker 认领、HTTP/TLS、配置与 core bridge 仍待接入，组件证据见 `../acceptance/installed-product-25-identity-foundation.md`。当前 loopback Gateway 的固定 bearer 不能被宣称为已完成网页登录。本模块新增 Web 认证与适配面，保持现有 Run/Control/Approval、Harness/Store trait 和 99 EventKinds。

## 1. 职责与边界

在受限 `forme-webd` 中实现可独立于核心启动的静态页面、认领、登录/session、配置控制、LAN TLS、正式 API/SSE 转发、附件/产物授权。持久认证状态独立于 brain store，只有 Web 写者。

认领、配置提交和服务启动与 prd/24 的 broker 对接；业务组件由 prd/26 实现。Web 不接 provider SDK、不调模型、不执行工具、不读 memory SQLite，也不安装上游服务端/插件系统。账号只支持单 owner，不能添加第二个拥有不同脑的普通用户。

## 2. 全局位置与集成契约

- gateway 实现 Web transport、认证 middleware、内部 Gateway 认证与安全 DTO adapter；protocol 保存跨进程 DTO；不新增 gateway→store/config/execution 边。
- 与 core 交互通过既有 Harness/Gateway 外观，最终事实仍由 core 写 forme-store。既有 `/v1/*` 保留内部 loopback；外部 `/api/v1/*` 的路由表是显式 allowlist，不做代理通配符。
- 与 broker 交互通过 prd/24 HostManagement；Web 只有有限 owner-maintenance 凭据，没有 AgentExecution 凭据/driver 权限。
- brain 发出的 Run/Action/Approval/Communication/DataLifecycle 等事件不变。登录、认领、配置 revision 等使用 architecture/10 §4 的管理/认证记录，不新造 EventKind。
- Web 用户的可信身份来自登录服务端上下文，不能由消息正文、OwnerId 字段或 forwarded header 构造。消息渠道仍使用既有 adapter/ParticipantProfile/授权，不因 owner 登录 Web 就把外部参与者升级成 owner。

## 3. 数据结构与类型

所有 wire/persistent 对象带 `schema_version: SchemaVersion`；secret input 是仅请求生命周期的敏感类型，禁止 Debug/Serialize 到日志、事件或持久审计。

| 对象 | 字段/约束 |
|---|---|
| `OwnerAccount` | `account_id, owner_id, login_name, password_hash, credential_revision, auth_epoch, created_at`。password hash 仅认证库内部，API 无此字段。OwnerId 在改名/改密码/恢复后不变。 |
| `LoginSession` | `session_digest, account_ref, owner_ref, issued_at, last_seen_at, idle_expires_at, absolute_expires_at, auth_epoch, csrf_digest, reauth_binding?`。随机 cookie token 不落库。 |
| `SetupSession` | `setup_digest, install_ref, claim_operation, expires_at, csrf_digest, verified_data_candidate_ref?`。15 分钟，仅初始化范围，完成认领后消耗。 |
| `ClaimRequest` | `install_id_safe_ref, verified_data_candidate_ref?, login_name, password, operation_id, idempotency_key`，由 setup session 认证。不允许调用者任意指定 OwnerId；已有脑按验证结果绑定。 |
| `ClaimTransaction` | `operation_id, install_ref, claim_digest, stage, reserved_owner_ref, account_ref, expected_revision`。跨 broker/Web 认领事务可恢复，原证明只用其摘要。 |
| `CoreSessionAssertion` | `install_ref, owner_id, session_digest, auth_epoch, audience, request_digest, expires_at, nonce, signature`。服务端签发/验证，不暴露浏览器。 |
| `ProductConfigSnapshot` | `revision, owner_binding_ref, model_profiles, channel_configs, workspace_refs, capability_options, proxy_config, network_config, effective_sources, active_ack`。读取不含密钥原文。 |
| `ConfigPatch` | `operation_id, idempotency_key, expected_revision, typed_changes, secret_mutations`。未声明字段和不允许 hot change 的字段拒绝或要求 restart。 |
| `ValidationReport` | `config_revision, check_results, blocking_issues, restart_required, current_capabilities, model_test_status`。静态配置校验不能自报真实模型可用。 |
| `WebCommandReceipt` | `command_ref, status, run_ref?, operation_ref?, pending_ref?, snapshot_revision?`。状态为 accepted/existing/conflict/blocked；accepted 不是 action success。 |
| `StreamResume` | `stream_scope, opaque_cursor, snapshot_watermark, schema_version`。run events 内部排序仍只认 stream_seq。 |
| `ApiProblem` | `code, safe_message, issue_ref, retryable, retry_after?, required_action?, correlation_ref`。拒绝返回 raw error stack、secret/path。 |

`ProductConfigSnapshot` 的 provider key/channel credential/内部 bridge secrets 只以设置状态与可安全显示的 metadata 表示。浏览器不可读的数据也不能藏在 JSON 的未展示字段中。

## 4. 接口与签名

### 4.1 外部 Web API

路径以 `/api/v1` 为根。所有 mutation 带稳定 `Idempotency-Key`，涉及版本对象带 expected revision；服务端用规范化完整语义核对，异义 key 冲突且零推进。请求 body 大小和上传能力按配置限制，不无限缓存。

| 路由 | 请求与结果 | 身份/语义 |
|---|---|---|
| `GET /bootstrap/status` | 仅 claimed/setup_needed/protocol_version。 | 公开最小信息；不返回 owner/path/model。 |
| `POST /bootstrap/prove` | 本机一次性证明 → 有限 setup cookie。 | 原子预留 claim；并发/重放不能创建第二个初始化者。 |
| `GET/POST /bootstrap/data` | 已识别数据候选 / 准备指定备份并验证。 | 只接受有效 setup cookie + CSRF；离线管理路径验证 owner/key/完整性，不允许匿名读取/任意文件路径。 |
| `POST /bootstrap/claim` | ClaimRequest → claim receipt/owner session。 | 有限 setup session + 已验证数据 ref + CAS；消耗证明/初始化会话。 |
| `POST /auth/login` | login/password → cookie + safe session view。 | 验证密码；登录也校验 Host/Origin 和 login CSRF。 |
| `GET /auth/session` | safe owner/account、expiry、csrf nonce。 | 有效 cookie；不返回内部 assertion。 |
| `POST /auth/logout` | 撤销当前会话。 | CSRF；不会取消已授权后台任务。 |
| `DELETE /auth/sessions` | 撤销全部会话并升 auth epoch。 | 近期重新认证；不更换 OwnerId/keys。 |
| `POST /auth/reauth` | password + operation digest → 绑定的短时 proof。 | 仅 5 分钟且单 operation 使用。 |
| `PUT /auth/password` | old/reauth + new password。 | 原子 hash/epoch 更新，旧会话撤销。 |
| `GET /product/status` | HostStatus + safe health。 | 登录；core 离线也可用。 |
| `GET/PATCH /product/config` | safe snapshot / ConfigPatch → validation/operation。 | CAS，secret 只写；apply/ack 独立。 |
| `POST /product/config/validate` | 指定 revision → ValidationReport。 | 不把格式检查等同于模型实测。 |
| `POST /product/commands` | typed MaintenanceCommand → OperationRef。 | 必需的 reauth + broker allowlist；不是 shell。 |
| `GET /product/operations/{id}` | 安全进度/receipt。 | 按 product cursor 重连，不读取 brain journal。 |
| `GET/POST /brain/runs`、`GET /brain/runs/{id}` | 正式 run list/submit/read。 | owner assertion；同一 ProductionRuntime。 |
| `GET /brain/runs/{id}/events` | 正式事件 SSE。 | run scope + cursor，heartbeat/重连；无隐藏 CoT。 |
| `POST /brain/runs/{id}/control` | 原有 typed cancel/resume/approval union。 | exact pending binding；不允许通用 retry unknown。 |
| `/brain/{sessions,workspaces,inbox,candidates,memory,settings}` | 逐项映射已有 read/command 外观。 | 缺少 core 外观时先做正式 adapter；不能临时直读 DB。 |
| `POST /brain/uploads` | 有限附件流 → scoped content ref。 | core content store/AEAD 路径；未确认附件不成为指令。 |
| `GET /artifacts/{opaque_ref}` | 授权内容/下载。 | owner/run/workspace scope；拒绝任意路径。 |

`/brain/*` 不是字符串转发；每一路由明确 input schema、command type、auth、版本和响应 projection。只有 static assets 与最小 bootstrap/login 路由允许未登录；public health 不暴露内部 issue 细节。API 客户端版本不兼容返回明确 UnsupportedVersion，而非丢弃未知安全字段继续。

### 4.2 内部接口

```rust
trait WebIdentityService {
    fn login(&self, request: SensitiveLoginRequest) -> Result<SessionCookie>;
    fn authenticate(&self, cookie: &SensitiveCookie,
                    request: WebRequestBinding) -> Result<AuthenticatedWebOwner>;
    fn revoke(&self, owner: &AuthenticatedWebOwner,
              command: RevokeSessions) -> Result<AuthRevision>;
}
trait CoreWebBridge {
    fn submit(&self, owner: &AuthenticatedWebOwner,
              command: BoundBrainCommand) -> Result<WebCommandReceipt>;
    fn snapshot(&self, owner: &AuthenticatedWebOwner,
                query: ScopedBrainQuery) -> Result<SafeProjection>;
    fn stream(&self, owner: &AuthenticatedWebOwner,
              query: StreamResume) -> Result<EventSubscription>;
}
```

`AuthenticatedWebOwner` 只能由认证 middleware 创建。bridge 为每次 mutation 生成 audience/request/session-bound 短时 assertion，内部 Gateway 重新验证服务身份与当前 session/epoch；必要的 introspection 走私有认证 IPC。注销后不能只等旧 token 到期才停止接受新 mutation，core 无法核实有效会话时拒绝。读长连接也周期确认会话，失效后结束连接，但已授权 run 不随之取消。

## 5. 行为、状态与并发

### 5.1 认领与口令恢复

未认领服务只有最小公开状态和证明交换；proof 的生成、期限和交付按 architecture/10 §3。prove 先 broker CAS 预留 installation，换取有限 setup session；在其 scope 内验证新脑/原脑/恢复候选后，claim 才绑定正确 OwnerId、以 Web transaction 创建账号 hash 并 broker commit。中断恢复根据双方 operation refs 核对；pending claim 只能由原 setup session/本机管理身份恢复或显式撤销，不能让另一个访问者补填密码夺取账户。

已有脑/备份的 owner 来自解密与完整性验证，不能先随机创建 owner 再修改它适配旧数据。认领完成或本机撤销/轮换证明使 setup session 无效。已认领账号恢复不能导入另一个 owner 的脑；owner 不匹配应拒绝，而不是改写账户/数据身份。

密码规则、Argon2id 和 TTL 统一引用 architecture/10 §5.1。改密码/本地恢复先写新 credential revision 与 revoke epoch，再提交最终状态；崩溃后不得重新激活旧口令。忘记口令通过本机管理员 `hostctl` 发起有限 reset 操作，记录事实、撤销会话，不建立匿名远程找回或重置 OwnerId。

### 5.2 配置与启动

初始化顺序：本机证明/有限 setup → 验证/选择脑数据 → 完成账号认领 → 配置模型/key/proxy → workspace/能力 → 静态 doctor → 受控启动 → runtime readiness → 可选正式模型连接测试。doctor 分清格式、密钥存在、网络可达、真实模型工具兼容；未执行的测试显示未验证。

保存通过 broker 版本事务：校验 patch/身份 → staging+secret resolver 写入 → validate → expected revision CAS → commit config → 通知 core 应用或排空重启 → 对照 actual composition ack。保存成功不是应用成功；失败保留 previous known-good，显示待修复字段且不把密钥回填页面。

服务配置优先规则按 architecture/10 §5.2。模型测试若需真实调用，只由正式 core/runtime 发起并记实际 profile/结果；Web 与 broker 不另建一个可直调 provider 的测试通道。错误 key 不进入无限重启；core 可进入 Degraded/ConfigurationRequired，Web 继续可达。

### 5.3 身份与审批

请求到达先 Host/Origin/CSRF/session/epoch 检查，再路由权限，再生成内部可信 AuthContext。浏览器 body 不接受 principal/approver 的自我声明；批准人由认证 owner 绑定。

批准操作只提交 pending approval ref、选择与预期 binding，服务端读取最终 plan 和规范化摘要；显示与执行的 affected_resources、schema/policy、nonce/expiry 必须相同。页面过期、plan 改变、能力撤销、切换 owner/epoch、已消费 nonce 均返回明确 conflict/expired，不能自动重新批准。L5 沿用 one-shot，不提供“此网页内总是允许”。

Unknown 呈现原 job/receipt/probe/裁决路径，不包装成普通重试按钮；取消请求只有在 backend 确认后才显示取消成功。control 与 resume 接收现有 PendingKind 对应 typed input。

### 5.4 SSE、断线与请求幂等

run SSE 依 `stream_seq` 恢复；收件箱/多 run 列表使用服务端 opaque projection cursor。snapshot 与 subscription 需共享 watermark，gap/retention 使客户端重取，不以时间过滤跳过事件。慢消费者有界队列与可恢复断开，不阻塞 core writer。

提交超时保留幂等 key/command ref；重连先查询已有 receipt，再同 key 重试，不能创建第二任务。只有未提交草稿可以本地持久化，已经接受的任务不在浏览器离线继续执行。

### 5.5 本地/LAN 与维护

默认 loopback:8765；LAN 变更先预检绑定地址、Host allowlist、证书私钥、HTTPS、firewall 范围，再写操作 intent，切换并自检。原连接失效时提供已验证新地址和恢复说明；失败回到可达的 loopback 修复状态，不能全接口裸 HTTP 兜底。

管理 RPC 必须验证 OS peer/服务身份。core 不可达时，Web 可以显示状态、改配置、检查指定备份并请求受控恢复；不能在 Web 进程自己打开脑数据库补读。离线完整性/恢复委托获得 exclusive lock 的既有维护路径。

## 6. 错误、安全、权限与审批

API 错误包括 `Unauthenticated / Forbidden / CsrfRejected / ClaimUnavailable / Conflict / PendingRestart / CoreUnavailable / Maintenance / CertificateUntrusted / RateLimited / UnsupportedVersion / RecoveryRequired`。HTTP 202 表示 accepted，409 表示陈旧版本/异义 key，429 带 Retry-After；不能把 200/202 作为动作完成证据。

登录/claim/reauth 均严格 body/速率限制，Argon2 工作队列有界。登录响应不暴露账号是否存在；不把登录失败原因写成含原口令的 stack。CSRF nonce 不放 URL；SameSite 不能替代 Origin/Host 检查。对 loopback 也防 DNS rebinding，拒绝未知 Host 与任意 CORS。

附件/Markdown/工具输出都为不可信内容；CSP、HTML 清洗、链接协议与 sandbox 由 prd/26 执行。下载按安全 ref/实际资源授权，拒绝路径穿越、符号链接替换、错误 MIME/script 执行；上传有限流/限量/临时文件保留与 crypto lifecycle。

所有错误区分宿主故障与 Agent FailureTaxonomy；只有经过 core 的失败才写其 FailureEvidence。provider keys、密码/session、helper credential、数据 key 不进入公共 DTO、localStorage、事件、日志、trace、诊断或分析 telemetry。

## 7. 配置与可观测

- `web.{bind,port,allowed_hosts,trusted_proxies,tls}`；初始端口 8765，LAN 必需 TLS。
- `auth.{idle_ttl,absolute_ttl,reauth_ttl,password_hash_parameters,login_rate,argon_concurrency}`；安全默认取 architecture/10。
- `transport.{read_rate,command_rate,event_connections,event_queue_limit,reconnect_policy,upload_limits}`；读/命令/登录独立 bucket。
- `service_config.{source_mode,revision,active_ack}`；保存、有效来源和需要重启均可见。

记录安全 request/operation refs、认证结果码、速率 bucket、断线/背压/gap 次数、core 状态和 config ack；不记录 cookie/authorization/body 秘密。日志 logout/reset 可核对，但不暴露原始 session 摘要作为可用 credential。

## 8. 实施步骤与顺序

1. 固定 DTO/route allowlist/错误码；与 prd/24 对齐 claim/config/operation 事务。
2. Web 独立启动与安全静态资源；无模型配置也能看到 bootstrap，公开面最小化。
3. 认领、Argon2id/session/CSRF/epoch/本机恢复；先做抢占、伪造与并发否定测试。
4. 持久 typed config + secret resolver + effective source + doctor/start/ack，不依赖用户手工 env。
5. 内部认证 bridge 与正式 Gateway 协议；验证 owner/approver 不可被 payload 替换。
6. LAN HTTPS/firewall/Host/证书信任；SSE/backpressure/idempotency 和附件授权。
7. 与 prd/26 五个工作面联调，完成主验 IP 和 IP24；前端 stub 不代替本模块后端完成。

## 9. 测试

单元：密码参数/上限、session expiry/epoch、CSRF/Host、claim CAS、config transaction/recovery、assertion audience/digest/nonce、异义 idempotency、stream watermark。fixture 自建，不引用真实账号/key。

集成：两浏览器并发 claim、断电阶段恢复、logout 后 mutation、伪造 owner/审批、core 停止/错误配置/key、环境覆盖、LAN 未信任证书、错 Host、重复提交、SSE gap、读流限流与命令不互相饿死。批准反向场景验证 driver=0，而不只看 HTTP 拒绝。

真实浏览器验证 cookie/CSRF/续接；原生平台验证账户/key 权限和网络规则；至少一次真实 model profile/工具调用通过既有 core 全链路。使用 IP04–09/IP13/IP21–22 的完整正反断言；跨平台未跑不标 PASS。

## 10. 验收

清洁安装无 env 即可认领、设账号和模型并启动原有大脑；重启/改密码保留脑/OwnerId，core 离线仍可受认证修复。LAN 无匿名/裸密钥，内部 Gateway 不被公网化，所有 control 绑定最终 plan 且幂等。

网页保存配置后能证明 actual active revision/provider；读流不会压垮命令配额。主验 IP 有真实证据，历史 governance/regression 不退化；本模块没有独立 model/backend/memory writer。

## 11. 原创性与合规

认证、密码哈希、TLS、cookie、CSRF 和 IPC 使用成熟公开协议/安全库，不自行设计密码学。新增依赖锁版本、来源、实际许可、transitive notice/漏洞/SBOM，实施时更新清单；本次文档不表示已引入。

不从 DeepSeek/OpenClaw/Hermes 复制后端认证、消息、RPC 或配置系统。前端 UI 的受限复用只按 [frontend-reuse-policy](../compliance/frontend-reuse-policy.md)，不能扩大到本模块 core/安全实现；合规门继续 fail closed。

### 11.1 实施细化：认证持久化与认领接缝

认证实现放在 gateway 的 `web::identity`，仅写独立 `web-auth/identity.sqlite`，使用独立 OS writer 锁、SQLite FULL 事务与 application/schema/installation binding。未知、旧版本、其他用途或损坏的文件拒绝接管，不初始化覆盖。此处复用 rusqlite 的公开事务 API，不新增 gateway→store 边，也不打开 brain SQLite。目录 ACL 由安装器负责，模块内的路径检查不自报已验证 OS 权限。

单 owner 创建分为 Web 准备账号承诺（随机 salt 的 Argon2id 记录）和消费 broker 的已提交认领回执。新增 `WebClaimCommit / SignedWebClaimCommit` 产品 DTO；签名绑定 install、operation、已验证 candidate、真实 owner、账号承诺和期限。验签只使用受管预置公钥，不接受浏览器带来的公钥；未接通 broker 的有限 setup/数据验证/commit 前，不开放匿名注册或把此组件当成 IP05 完成。断线可用原 operation 和原承诺幂等收尾，不生成另一 owner。

密码 KDF 固定使用 Argon2id v19、64 MiB、3 iterations、parallelism 1、16-byte OS salt 和 32-byte 输出，持久记录显式保存 schema/算法/参数；不导入任意参数执行。密码工作最多并行两个，正在运行的计算持有名额到真正结束。密码/原始 cookie 类型不派生 Debug/Serialize 并在释放时清零；会话和 reauth 原文只交 transport，数据库仅保存摘要。

会话默认 idle 30 分钟、absolute 12 小时；CSRF nonce 从随机 HttpOnly session credential 以独立域摘要派生，可在刷新后返回，不存 plaintext cookie。任何 mutation 仍需 transport 的精确 Host/Origin/CSRF 和当前 epoch 重查。5 分钟 reauth 必须绑定具体 operation；改密码的 operation 绑定预生成新哈希承诺，全会话撤销使用不同的操作域，不能互换或替换新口令。账号/epoch/一次性 proof 消耗与会话撤销在同一事务完成。

这些是原有 §3–5 的实现细节，不更改既有 Harness/Store/WebIdentityService 语义，不增脑事件。组件证据与 Web HTTP/浏览器、broker 双边恢复和原生安装 IP 证据分别记录。
