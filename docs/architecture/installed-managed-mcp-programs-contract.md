# 受管 MCP 程序注册实施契约

本补充落实 owner 已批准的受管接入方案中的 Host 程序注册，不改变 ActionParameters、WorkerJob/Permit、EventKind、crate 图或 unsafe 例外。注册不是执行许可，注册/读取/撤销不启动 MCP，也不写脑事件。

## 1. 固定来源和首个可执行 profile

Host 从独立受保护的本机来源配置读取程序候选；Web、模型与 ActionIntent 只能提交 opaque source_ref，不接受路径、argv、环境或凭据。来源配置本身只由安装/本机管理域预置，不通过注册接口上传或安装软件。每个来源绑定固定 scope、sandbox_profile_ref、绝对 exe、argv 和期望内容摘要。profile 固定为 `windows-appcontainer-mcp-stdio-v1`，不接受请求选择其他隔离策略。完整 argv 最多 64 项，每项最多 4096 UTF-8 bytes、总量最多 16384 bytes，不含控制字符；程序文件最多 256 MiB，拒绝 shell、链接、reparse、硬链接、多次重名和路径别名。

这里定义首个受管实现单元，不取消原提案中外部资源、凭据及其他消费者的后续义务。首个原生 profile 只支持独立可执行的本地 stdio server：无网络、无派生进程、无外部文件和凭据；资源范围和 credential refs 固定为空且参加注册摘要。需要解释器、外部依赖目录、网络、资源或凭据的候选明确拒绝，不能当作已支持。后续 profile 继续在原批准的有限资源/凭据范围内细化，不能通过空列表表示无限访问。

## 2. 注册请求与视图

新 closed/versioned 对象均 schema_version=1。所有 opaque ref 使用 validate_product_id，摘要使用完整 sha256。

- ManagedMcpProgramCommand：schema_version、installation_ref、owner_ref、authority_epoch、operation_id、expected_revision、requested_deadline、action。
- ManagedMcpProgramAction：Register{source_ref,source_digest} 或 Revoke{program_ref,registration_digest}，deny_unknown_fields。
- ManagedMcpProgramChoice：schema_version、source_ref、source_digest、scope、sandbox_profile_ref。
- ManagedMcpProgramView：schema_version、program_ref、registration_digest、source_ref、scope、sandbox_profile_ref、revoked。
- ManagedMcpProgramCatalog：schema_version、installation_ref、owner_ref、authority_epoch、revision、choices、programs。
- ManagedMcpProgramOperation：schema_version、operation_id、command_digest、revision、program。

command_digest = canonical_digest(("forme.managed-mcp.program.command.v1", 完整 command))。请求至多 8192 bytes，期限为当前时间之后且剩余不超过五分钟。Register 绑定当前实际候选摘要；Revoke 绑定确切原注册。相同 operation/digest 只读原回执，异义复用拒绝。校验 owner/epoch、schema、期限、CAS 和候选必须在任何注册事实追加前完成。视图不返回程序路径、argv、凭据或文件正文。

## 3. 原件、摘要与恢复

Host 使用只读句柄打开实际程序，Windows share mode 不授予 write/delete，并持有完整祖先目录，拒绝链接与多 hardlink。实际身份由既有 audited AppContainer ACL adapter 的 volume/file id 取得，不使用路径、mtime 或文件长度冒充身份。内容按有界流计算 SHA-256；候选摘要绑定源定义、实际文件身份、长度、完整内容摘要和固定无网络/无外部资源/无凭据 profile。

registration_digest = canonical_digest(("forme.managed-mcp.program.registration.v1", installation_ref, owner_ref, authority_epoch, operation_id, 完整已验证来源原件))；program_ref 为 mcp-program: 加该摘要的 hex。同名来源更换身份、内容、argv、scope 或 profile 后不能沿用旧 registration。已撤销 program_ref 永远不复活；显式重新注册得到不同 program_ref。

独立 Host append-only journal 使用 OS 独占锁、连续序号、previous/hash 链和 fsync；记录 Bound、Registered、Revoked，保留原命令及原注册。最多 4096 操作、64 MiB 日志、每条 64 KiB，超过或部分尾部均拒绝；不截尾修复。追加或 fsync 不明时当前 writer poisoned，拒绝后续操作，重启只读取原件，不自动重注册。产品配置、脑状态和包生命周期仍各用原权威。

执行前解析只返回持有真实文件及祖先句柄的注册 lease，核对当前源定义、文件身份/内容、scope/profile 和未撤销状态。lease 存活期间禁止程序替换；撤销必须由同一 registry writer 的检查线性化，并在每个后续 RPC 出口可见。未知/丢失/旧 epoch/错 owner/撤销/来源漂移均保持 unavailable，读取与重启零进程创建。

## 4. 认证与接线

Host 管理入口只接收 Web Configuration 域，Web 验证登录 owner、Origin/CSRF、幂等 header 和绑定完整 command digest 的近期口令重新认证，口令不传 Host。register/revoke 不从 AgentExecution 暴露。注册表在首次 owner claim 后绑定，同一 Host 与 worker 共享实例；锁序为 config -> worker -> registry，不在持 registry 锁时等待 config。

有限 HTTP 路径为 GET /api/v1/product/mcp/programs、POST /api/v1/product/mcp/programs/commands、GET /api/v1/product/mcp/programs/operations/{id}；内部使用对应 /v1/host/configuration/mcp/programs 路径。operation 查询不得执行请求；不开放通配代理。

## 5. 验证边界

协议正反和真实 Windows 文件验证包括：同字节替换文件也失效、内容/argv/scope/profile 漂移、硬链接/reparse/祖先替换拒绝、持 lease 时写入和删除被 OS 拒绝、原件重启和单 writer、撤销不可复活、错 owner/epoch/CAS/期限、异义操作、损坏/截断 journal、未认证及跨域请求。配置和 registry 构造/读取/注册/恢复均不得启动进程。

来源集合限定最多 64 项，程序总长度不超过 512 MiB；来源根与 journal、所有产品保护路径和 workspace 均不得重叠。该校验不能代替安装时的原生目录 ACL 核验。

Host 私有来源结构 `ManagedMcpProgramSource{schema_version,source_ref,scope,sandbox_profile_ref,program,args,expected_program_digest}` 是闭合对象，最多 64 项，完整来源配置至多 256 KiB。程序根必须是本机管理域预置的独立目录，不能位于 workspace、brain、keys、journal 或 worker runtime 内；同一目录的附加 DLL/解释器依赖不因此获得权限。当前只实现 Windows 真实文件 custody，非 Windows 不伪造等价保护。

所有视图和原操作查询在返回前检查 journal healthy；Register 原回执是历史事实，当前 revoked 状态以 catalog 为准，不能用原回执重新启用。重放校验注册摘要、program_ref、原命令 owner/CAS 和 Registered/Revoke 转移，不因 hash 链正确就信任不合法状态。注册命令失败不自动回滚或重放任何外部程序，因为本接口本身没有启动副作用。

本模块通过不表示 AppContainer discovery/call、Core metadata、Web 设置界面、其他 profile 或完整 IP 已完成。现有 MCP enabled 消费者保护在实际链路接通前保持。
