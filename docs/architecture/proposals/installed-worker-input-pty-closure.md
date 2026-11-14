# 安装 worker 输入与生产 PTY 的窄范围补充（待确认）

状态：**提案，尚未实施，未授权生产 worker 或本机 PTY 放开**。

本文只处理当前设计中剩余的两个安装侧接缝：`ContentRef`/`SecretRef` 作为受管 worker 输入，以及真实生产 PTY profile。它不修改已有的受管 MCP、插件、渠道、桌面授权或截图协议，也不把组件级 PTY 机制证据提升为产品能力。

## 1. 当前事实与缺口

- `forme-protocol` 已有 `ExternalInput::{Literal, Content, Secret}`、`ContentRef`、`SecretRef` 和冻结的 `WorkerExecutionSpec`。这些类型只表达最终计划中的引用，**不**证明安装 Host 有安全解析、撤销、期限或清理接缝。
- Core 的开发装配仍使用 `InMemoryContentResolver`；正式安装 worker 没有通用的内容读取入口。渠道代理是一个独立的、只针对固定 loopback 消息的例外，不能泛化为 Browser、Computer 或 PTY 输入。
- 安装桌面 helper 当前明确拒绝 ContentRef/SecretRef；这是正确的 fail-closed 行为。把内存 resolver 或 Brain 的凭据直接放进 helper 会绕过 executor-local、scope-bound 规则。
- `PtyBackend` 已有进程内输入、secret redaction 和组件级 ConPTY/Job/AppContainer 机制测试，但没有生产 `sandbox:installed-pty-v1` 的固定 profile、真实 worker 输入 grant、真实服务身份和完整进程树证据。

## 2. 不变的边界

1. 不修改 `ExternalInput` 的既有字节形状，不把 `SecretRef` 替换成 Literal，不增加脑 `EventKind`、crate 依赖边或 unsafe 文件。
2. 输入正文、secret 值、`SecretRef` identifier、文件路径和 provider 凭据不进入 journal、事件、日志、trace、manifest、Web body 或验收工件。
3. Core 仍是最终 plan/permit 的唯一来源；Host 只接受与原 `job_id/action_ref/plan_digest/normalized_action_digest/authority_epoch/nonce/deadline/workspace_scope` 完全绑定的一次性输入授权。
4. 输入解析失败、撤销、过期、版本漂移、读取结果不明或清理不明都保持原 job `Unknown`，不改成空输入、不重派、不用新 job 猜测原结果。
5. 生产安装缺少真实 resolver、OS 隔离或 worker binding 时，相关 capability 继续 `Unavailable/Blocking`；开发进程内 backend 只用于测试。

## 3. 受管输入授权（schema 1）

新增独立 closed DTO，挂在 `WorkerDispatchRequest` 的可选 `input_grants` 字段中；旧 Host 遇到该字段或未知版本必须拒绝，不能忽略后按 Literal 执行。

### 3.1 `WorkerInputGrantV1`

字段固定为：

`schema_version, grant_ref, installation_ref, owner_ref, job_id, action_ref, plan_digest, normalized_action_digest, permit_digest, authority_epoch, nonce, workspace_scope, input_path, input_kind, content_ref, content_digest, content_bytes, secret_slot_ref, issued_at, expires_at`。

- `input_path` 只允许已知的 closed path（当前仅 `computer.type`, `browser.type`, `pty.stdin`, `pty.environment` 和 `app_api.body`）；不接受模型提供的字段名或 JSON pointer。
- `input_kind=Content` 时必须绑定不可变 `content_ref`、原始字节摘要和长度上限；正文通过 AgentExecution peer 的一次性、分块、只读读取获得。每个 chunk 重新核对 binding、连续 offset、总长度和整体摘要，最大总量取最终 plan budget 与 16 MiB 的较小值。
- `input_kind=Secret` 时只携带 Host 已登记的 opaque `secret_slot_ref`，不携带 `SecretRef` identifier 或明文。slot 必须在安装受保护配置中绑定一个 executor-local SecretRef、允许的 input path、owner/workspace、用途和过期时间；Host 在 worker 启动前最后一跳解析，解析结果只进入受限 stdin/environment 管道，不能写入 job 文件、日志或普通环境继承。
- 一个 action 的每个输入只允许一份 grant；`grant_ref`、`nonce` 和 `input_path` 不得跨 job、跨 plan、跨 workspace 或跨 participant 复用。相同 job 重启只能查询原 grant/读取状态，不能新签一份替代 grant。
- Content 读取和 Secret slot 解析均在实际副作用前完成；已 `Started` 后结果不明仍按原 worker/desktop/PTY Unknown 处理，不能用“解析成功”证明动作已发生。

### 3.2 解析与清理顺序

1. Core 在最终 plan recheck 后，根据真实内容 scope 或 Host 已登记 slot 生成 `WorkerInputGrantV1`，并把 grant digest 纳入原执行 binding；浏览器/Web 不能自报 ref、slot、scope 或 bytes。
2. Host 以自身 AgentExecution 身份核对 grant、原 worker job/permit、当前配置 revision、来源/撤销/生命周期 fence 和剩余 deadline，再决定是否读取正文或解析 slot。
3. Host 将 Content 正文送入受限 worker 输入管道；Secret 只通过一次性 slot materialization 交给固定 worker。worker 不可读取 Core DB、Host key 目录、owner 环境或其他 job 的输入。
4. 动作完成或明确失败后，Host 先清理 worker 输入副本和 secret materialization，再落原 manifest/receipt；清理不明保留 Unknown 并禁止新准入。删除、logout、撤销和恢复先阻断读取，再按原 data fence 处理所有副本。

## 4. 生产 PTY profile

新增受管 profile `sandbox:installed-pty-v1`，只允许在已有 AgentExecution worker isolation 和已验收桌面/会话边界内使用：

- 固定的仓库自建 runner、固定 `portable-pty` 版本、固定程序 allowlist、固定 workspace overlay 和无继承环境；不接受任意 argv、PATH、代理、credential 环境或模型指定的可执行路径。
- Windows 必须先取得本 job Low AppContainer SID、低完整性 token、kill-on-close Job Object 和真实 ConPTY 进程树观察，再 resume 首次用户输入；任何一步无法独立回读时在 `Started` 前拒绝。Linux/macOS 继续按现有未发布边界，不能用 WSL、Docker 或交叉编译代替原生证据。
- `pty.stdin` 与 `pty.environment` 只能引用本提案的 Content grant 或 Secret slot；Literal 只保留开发/机制测试，不进入生产 managed profile。输出必须继续有界、逐 chunk、跨 chunk redaction；发现可能的 secret 回显、编码/终端状态异常、后代未收束或退出不明时保持 Unknown。
- PTY 不获得桌面 helper 的完整用户权限，不访问 brain/Host journal/key 目录，不创建第二个 writer，不通过渠道代理或浏览器 listener 出网。网络能力默认为拒绝；需要任何网络 grant 必须另提 exact-scope 协议。
- `PtyActionSpec` 的程序、cwd、cols/rows、输入 grant digest、timeout 和 output budget 共同参与原 `normalized_action_digest`；任何变更都使原审批失效并要求新 plan。

## 5. 必须回填的正式文档与协议测试

在代码前，需要把上述 exact scope 回填到：

- `docs/architecture/canonical-contract.md` §29 的输入解析、executor-local slot 和生产 PTY profile 条款；
- `docs/prd/24-installer-and-service-lifecycle.md` §4.1.2–§4.1.4 的 Worker DTO、AgentExecution 读取顺序、清理和 native admission；
- `docs/architecture/10-installed-product-and-web-workbench-architecture.md` §7.1.2/§7.1.3 的输入与 PTY 生产边界；
- `docs/requirements/18-installed-product-verification-strategy.md` 的 IP10/IP11/IP17/IP18/IP21/IP24 输入、secret redaction、PTY 进程树和删除证据。

实现前必须补齐 closed serde/golden 测试、grant digest/CAS/expiry/revoke、chunk/size/offset、跨 owner/workspace/path、未知字段/版本、secret 不落盘、同号重启、清理失败和零重派场景。随后才实现 Host 解析、固定 worker 接线和真实 Windows native receipt；组件/机制测试不关闭 IP10/IP11/IP24。

## 6. 本次请求的最小确认

请确认是否批准本文 §3 的受管 Content/Secret 输入授权，以及 §4 的 `sandbox:installed-pty-v1` 生产 profile。批准后先提交上述正式文档变更，再按“协议 → Host resolver → worker 接线 → native 验收”的顺序实施；未确认前只保持现有拒绝路径。
