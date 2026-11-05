# 工程质量与可维护性规范

本规范承接 `prd/01 §9–10`、`canonical-contract` 和模块 PRD。适用于 Rust 内核、产品宿主、Web、构建与测试；不改变冻结协议、18-crate 依赖图或验收语义。代码审查首先判断职责、行为和治理是否正确，格式与行数只是辅助信号。

## 1. 依据和适用方式

研究日期：2026-10-22。采用以下公开规范的原则，不复制其实现或测试：

| 依据 | 在 forme 中的落点 |
|---|---|
| [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html) | 一致命名、强类型、可预测接口、错误语义、最小公开面、rustdoc 的错误和副作用说明。秘密类型不因通用建议而派生 Debug/Serialize。 |
| [Rust Style Guide](https://doc.rust-lang.org/style-guide/) | 默认 rustfmt，避免个人格式争论；模块按职责组织。 |
| [Clippy 使用与 lint 分组](https://doc.rust-lang.org/stable/clippy/usage.html) / [配置](https://doc.rust-lang.org/clippy/lint_configuration.html) | strict Clippy 是阻断门；复杂度/参数过多提示用于审查，不全局启用 pedantic/restriction 后批量 allow。 |
| [Cargo Manifest 与 Rust 版本](https://doc.rust-lang.org/cargo/reference/manifest.html) | 锁定 edition/MSRV/依赖来源和发布元数据；不把尚不稳定的 Cargo lint 配置误当作当前强制门。 |
| [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown) 和 [select cancellation safety](https://docs.rs/tokio/latest/tokio/macro.select.html) | 明确停止信号、停止准入、等待已启动工作；请求取消不代表 blocking 工作停止。 |
| [OWASP ASVS](https://owasp.org/www-project-application-security-verification-standard/) | Web 身份、会话、输入、访问控制、密钥、浏览器安全和错误处理的审查分类；不自称已取得 ASVS 认证。 |

文件 600 行、函数 80 行之类不是 Rust 官方标准。本仓库的规模阈值是维护约定，不是正确性证明或行业认证。

## 1.1 四级判定标准

代码评审和自动门按四级判定，不能用低级结果掩盖高级问题：

| 级别 | 判定问题 | 形式 |
|---|---|---|
| P0 正确性与合规 | 能否编译、回归、通过依赖/原创/unsafe/协议门？ | 自动阻断。 |
| P1 架构与治理 | 是否仍沿冻结 crate 图、唯一事实源、执行前重查、审批/能力门和 Unknown 语义？ | 变更审查 + 场景测试；违反即阻断。 |
| P2 可维护性 | 一个模块是否只有一个主要变化原因，跨层接口是否窄，规模是否按棘轮下降？ | 自动规模/依赖门 + 代码审查。 |
| P3 产品行为 | 真实 HTTP、进程、浏览器、系统服务和恢复是否达到对应 IP/S/C？ | 独立验收证据；不能用单元测试代替。 |

“测试通过”只证明覆盖到的行为没有失败；“代码质量通过”只证明 P0/P2 的自动部分通过。任何提交都必须明确哪些 P3 证据仍未取得。

## 2. 模块与规模

- crate 依赖只能来自 `architecture/03 §1`；拆文件不等于新建 crate。产品 DTO 与脑 EventKind 分域。
- `lib.rs`/`mod.rs` 以入口、公开重导出和少量共同类型为主。服务 transport、认证、配置、投影、平台适配与业务操作分别放模块。
- 新增 Rust/TS/JS 生产文件原则上不超过 **600 个物理行**；测试文件不超过 **1200 行**。CSS/脚本同用 600 行生产阈值。生成文件与 lockfile 不参与该指标，但必须可重复生成、审计来源，不得把手写代码伪装为生成文件。
- 已有超限文件记录在 `tools/quality-baseline.json`，按文件实施不增长棘轮。新增超限文件或旧文件增长阻断；删除/缩小旧文件无需更新基线。基线只能在明确审查理由后手工调整，不提供自动“刷新变绿”命令。
- 目标是单一变化原因和可独立测试的边界。不得为行数引入 `include!` 拼文件、一个函数任意切段、无意义 trait 或只转发的空壳模块。
- 函数超过约 80 行、分支过深、参数超过 7 个、生产函数同时处理认证/IO/状态迁移，是拆分审查信号；先看原子事务和审计完整性，必要时保留有解释的长函数。
- 新增或修改跨层代码时，审查必须标出：入口适配、校验/授权、领域状态迁移、持久化、外部副作用、响应映射分别由谁负责；同一函数同时拥有其中三类以上职责时，默认要求拆分或写出保留理由。
- 模块私有优先；跨模块使用最窄 `pub(super)`/`pub(crate)`。不得以拆分为由把全部内部状态公开。

### 2.1 大模块拆分的完成定义

拆分一个大文件只有同时满足以下条件才算完成：

1. public API、serde 字段/顺序、EventKind、事务边界和错误语义不变，或先完成对应文档变更。
2. 每个新模块有单一职责和明确 owner；只读查询、状态 fold、外部 IO、平台适配和测试 fixture 不混在同一模块。
3. 原有正反场景继续运行，并至少新增一个证明边界没有旁路的测试；不以“新测试通过”替代旧测试。
4. 生成文件、依赖图、许可证和 release manifest 没有未经说明的变化。
5. 规模债务减少或保持不增长，提交消息只描述本次拆分，不与安装、协议重设计或行为改动混在一起。

## 3. Rust 接口、错误与状态

- 协议 DTO 必须版本化，封闭安全输入拒绝未知字段。身份、路径、摘要、cursor、版本和 operation 使用既有强类型；不可用任意 JSON/字符串命令绕过 typed allowlist。
- public/跨边界函数说明前置条件、副作用、幂等、失败与 Unknown；返回值区分 accepted、committed、verified 与 applied。
- 对外安全错误码和诊断 ref；底层错误只在受控诊断中关联。不向用户回传密钥、body、cookie、私有路径或 provider 原始响应。
- 生产可失败输入不使用 unwrap/expect/panic；测试中的断言可以使用。禁止吞错后返回假成功、默认空脑或换身份继续。
- 状态改变必须有唯一 owner；脑状态 command→event→fold，产品管理 journal 与认证事务独立。稳定事实不能只写内存 cache。
- 解析校验、计划绑定、执行前重查、nonce claim、动作和 receipt 按契约保持顺序。拆分不得打断事务或放松权限。

## 4. 并发、资源与恢复

- 每个 task/thread/child 明确 supervisor、停止信号、join/drain、期限、队列和错误去向；不得无管理地 fire-and-forget。
- async handler 不持同步锁跨 await；昂贵密码计算/阻塞 IO 使用有界工作队列。timeout/取消后运行中的 blocking 工作仍占其名额，直到真正结束。
- 事件、认证、产品 journal 各有唯一 writer；跨进程独占锁不可由进程内 mutex 替代。CAS、幂等键、grant/nonce 的持久性按协议验收。
- 任何副作用不明只探测原操作/取 receipt/等待裁决，不重新派发。服务重启/升级不能改写历史终态或丢失 Unknown。
- 所有外部读取、上传、输出、流队列、hash/密码并发、重试与日志有界；不同流量分类独立限流。限制用于资源与失控保护，不设认知消费硬封顶。

## 5. Web 与宿主

- Web 只用正式认证 API；不调用模型、工具或脑库。页面、API client、session、reducer、业务组件按职责分开。
- cookie HttpOnly、Host/Origin/CSRF、登录/注销/epoch、近期重认证在服务器 enforce。敏感 UI 缓存只在内存；退出、失效、BFCache 恢复先重新核验身份。
- 密码使用成熟 Argon2id；随机秘密来自 OS entropy；摘要/签名/AEAD 使用成熟库。禁止自研加密或把数据 key 派生自唯一登录口令。
- 安装及管理与 AgentExecution 分域；所有平台操作有限 allowlist、绝对程序路径、闭合参数、最小环境。未验证强隔离时 blocked，不以普通子进程冒充沙箱。
- Rust crate 默认保持 `#![forbid(unsafe_code)]`。唯一例外是 `crates/execution/src/installed/windows_appcontainer_acl.rs`、`windows_appcontainer_native.rs`、`windows_agent_execution_native.rs`、`windows_desktop_session_native.rs` 与 `windows_desktop_helper_peer_native.rs` 五个审计 Win32 adapter：`execution` 根和 `installed` 模块改用 `deny(unsafe_code)`，五个文件只允许围绕各自的 `windows-sys` FFI 边界做最小封装。未列入 canonical §29.9 的 Win32 调用、第六个 `allow(unsafe_code)` 文件或把例外扩到整个 crate 都是阻断项；这些 adapter 仍须通过对应的 token/ACL/Job/pipe/session/恢复证据，不能仅因存在代码而宣称 IP10/IP11/IP24。
- 第三方前端仅按 `frontend-reuse-policy.md` 逐文件准入；依赖 exact pin、lockfile、NOTICE/SBOM 与漏洞审计。不得批量复制上游再扩大豁免。
- 组件提供 loading/empty/error/conflict/offline/unknown；键盘、窄屏、色彩对比与 reduced motion 需真实浏览器检查。

## 6. 测试与审查

- 单元验证纯算法/状态迁移；集成验证真实存储/进程/HTTP 边界；平台验收验证原生服务/账户/ACL；产品验收用 IP01–IP24。层次不能互相冒充。
- 每个承重变化至少一个正常场景和对应否定/故障场景；验证事件/版本/driver 次数/持久化，不只看回答或 HTTP 200。
- 重构保留 public API、serde 语义、EventKind 顺序、事务、验证路径与现有测试。测试按领域拆，不重写成只验证新实现的镜像断言。
- 审查依次问：实现符合哪个 PRD？事实与执行 owner 是谁？失败/取消/重启怎样？是否可能越权/泄密/重复副作用？边界是否易测？是否新增可替代的抽象？

## 7. 执行门与维护债务

构建资源与缓存生命周期见 [build-resources.md](build-resources.md)。默认 dev/test 配置、卷剩余空间检查和缓存预算报告与本节执行门配套；资源维护不减少测试集合或放宽验收。

本地和 CI 运行 `py -3 tools/code-quality.py`；它检查冻结依赖图、源文件规模棘轮、crate unsafe 禁令和 scope，不取代 Rust 编译器/Clippy/合规门。`py -3 -m unittest tools.tests.test_code_quality` 验证检查器自身的拒绝路径。

每次实现提交仍须 `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、`cargo test --workspace --all-targets`、`bash tools/compliance-doctor.sh`；strict Clippy 与相应永久/产品门按 AGENTS 执行。新增工具不会跳过任何已有门。

`cargo audit`/release audit 继续处理可达漏洞，合规门处理许可证和来源。`cargo deny`/nextest 可以在单独兼容验证后采用，目前不宣称已经接入，也不为引入工具更改冻结测试过滤语义。

统一入口 `pwsh -File tools/verify-code-quality.ps1` 同时执行上述 Rust 门、结构检查器回归、`npm ci --prefix frontend --ignore-scripts`、`npm audit --prefix frontend --audit-level=moderate`、前端 strict TypeScript/行为测试、`tools/build-web.mjs --check` 和 bundle 完整性/保留边界回归。前端构建只在开发/CI 运行 Node；安装后由 `forme-webd` 提供编译嵌入的固定资源，不需要用户安装 Node。修改前端源码后先执行 `node tools/build-web.mjs`，提交生成的精确资源表、manifest、依赖清单与许可证；陈旧/额外资源必须阻断。托管 CI 仅可显式允许缺少本地研究语料，不放宽其他检查。

### 7.1 嵌套验收的共享检查归属

M0–M3 与 M5 的历史场景和 golden 保留；嵌套运行时，fmt、workspace check/test、strict Clippy、Python discover 和完整合规由同一轮的顶层门统一执行，避免同一源码重复跑完整工作区。M3 默认负责其共享检查，M5 负责其自身整门的共享检查；独立运行 M0/M1/M2 继续沿原链取得所需检查。

M0/M1/M2 的内部组合参数 `-DeferSharedChecks` 只将共享检查交给父门，所有命名场景、真实 golden 和独立工件核对继续执行。该模式明确输出共享检查尚未执行，不能输出该里程碑的完整验收 PASS。M3 仅在 `-RegressionOnly -DeferSharedChecks` 组合下接受同样委托；单独传 defer 必须在执行前拒绝。顶层 M5 不提供跳过共享检查的参数，任一共享检查失败均不得输出最终 PASS。

该组合只在一次实际运行中去重，不使用环境变量、旧回执或其他源码的结果跳过验证；不减少 S1–S99、C/IP、默认 workspace 测试或负向断言。门的编排测试须覆盖默认独立路径、委托传播、局部结果标识、实际共享执行次数及失败阻断，不能只检查脚本文字包含了命令。

已有大模块按风险分批拆分：先 gateway transport/控制面，再 harness orchestration/production，再 store 按事务领域拆。每批保持行为不变并独立提交，避免在安装/认证实现中夹带整库重写。规模基线仅表示已知债务，不表示这些文件已通过可维护性验收。

安装代码也纳入结构门：`packaging/` 的 PowerShell module、安装入口与 Inno 脚本按同一规模规则检查。Windows 统一门额外构建当前 hostctl 并运行真实临时 NTFS 权限/继承/替换与拒绝场景；它不注册服务、不使用真实 owner 数据，不能替代管理员安装和服务 token 验收。

本机有已审查的 Inno 编译器时，统一门另传 `-WindowsInstallerToolchain <absolute-toolchain-json>`，运行实际安装 helper 的低权限原生成功/失败夹具，证明 post-install 失败返回非零。它不安装 ForMe、不注册服务、不请求 UAC；未提供工具链时明确记为 `NOT_RUN`，不能用结构断言替代这份原生证据。

当前公开维护债务约 63 个文件。已完成的第一批是 `harness/src/brain_orchestration.rs`：路线状态机仍留在原模块，节点构造/输出归并/验证/签名转换已移到 `brain_orchestration/support.rs`，保留原有 API、事件和恢复语义，并移除已偿还的规模豁免。下一批优先级固定为：`harness/src/lib.rs` 的入口/控制域、`harness/src/production.rs` 的 composition/服务域、`store/src/sqlite.rs` 的事务/投影域；每次只处理一个事务边界。`memory`、`protocol` 等承重协议文件除非有明确收益，不做机械按行切割。
