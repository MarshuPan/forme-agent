# 工程质量与可维护性规范

本规范承接 `prd/01 §9–10`、`canonical-contract` 和模块 PRD。适用于 Rust 内核、产品宿主、Web、构建与测试；不改变冻结协议、18-crate 依赖图或验收语义。代码审查首先判断职责、行为和治理是否正确，格式与行数只是辅助信号。

## 1. 依据和适用方式

研究日期：2026-10-21。采用以下公开规范的原则，不复制其实现或测试：

| 依据 | 在 forme 中的落点 |
|---|---|
| [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html) | 一致命名、强类型、可预测接口、错误语义、最小公开面、rustdoc 的错误和副作用说明。秘密类型不因通用建议而派生 Debug/Serialize。 |
| [Rust Style Guide](https://doc.rust-lang.org/style-guide/) | 默认 rustfmt，避免个人格式争论；模块按职责组织。 |
| [Clippy 配置](https://doc.rust-lang.org/clippy/lint_configuration.html) | strict Clippy 是阻断门；复杂度/参数过多提示用于审查，不全局启用 pedantic 后批量 allow。 |
| [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown) 和 [select cancellation safety](https://docs.rs/tokio/latest/tokio/macro.select.html) | 明确停止信号、停止准入、等待已启动工作；请求取消不代表 blocking 工作停止。 |
| [OWASP ASVS](https://owasp.org/www-project-application-security-verification-standard/) | Web 身份、会话、输入、访问控制、密钥、浏览器安全和错误处理的审查分类；不自称已取得 ASVS 认证。 |

文件 600 行、函数 80 行之类不是 Rust 官方标准。本仓库的规模阈值是维护约定，不是正确性证明或行业认证。

## 2. 模块与规模

- crate 依赖只能来自 `architecture/03 §1`；拆文件不等于新建 crate。产品 DTO 与脑 EventKind 分域。
- `lib.rs`/`mod.rs` 以入口、公开重导出和少量共同类型为主。服务 transport、认证、配置、投影、平台适配与业务操作分别放模块。
- 新增 Rust/TS/JS 生产文件原则上不超过 **600 个物理行**；测试文件不超过 **1200 行**。CSS/脚本同用 600 行生产阈值。生成文件与 lockfile 不参与该指标，但必须可重复生成、审计来源，不得把手写代码伪装为生成文件。
- 已有超限文件记录在 `tools/quality-baseline.json`，按文件实施不增长棘轮。新增超限文件或旧文件增长阻断；删除/缩小旧文件无需更新基线。基线只能在明确审查理由后手工调整，不提供自动“刷新变绿”命令。
- 目标是单一变化原因和可独立测试的边界。不得为行数引入 `include!` 拼文件、一个函数任意切段、无意义 trait 或只转发的空壳模块。
- 函数超过约 80 行、分支过深、参数超过 7 个、生产函数同时处理认证/IO/状态迁移，是拆分审查信号；先看原子事务和审计完整性，必要时保留有解释的长函数。
- 模块私有优先；跨模块使用最窄 `pub(super)`/`pub(crate)`。不得以拆分为由把全部内部状态公开。

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
- 第三方前端仅按 `frontend-reuse-policy.md` 逐文件准入；依赖 exact pin、lockfile、NOTICE/SBOM 与漏洞审计。不得批量复制上游再扩大豁免。
- 组件提供 loading/empty/error/conflict/offline/unknown；键盘、窄屏、色彩对比与 reduced motion 需真实浏览器检查。

## 6. 测试与审查

- 单元验证纯算法/状态迁移；集成验证真实存储/进程/HTTP 边界；平台验收验证原生服务/账户/ACL；产品验收用 IP01–IP24。层次不能互相冒充。
- 每个承重变化至少一个正常场景和对应否定/故障场景；验证事件/版本/driver 次数/持久化，不只看回答或 HTTP 200。
- 重构保留 public API、serde 语义、EventKind 顺序、事务、验证路径与现有测试。测试按领域拆，不重写成只验证新实现的镜像断言。
- 审查依次问：实现符合哪个 PRD？事实与执行 owner 是谁？失败/取消/重启怎样？是否可能越权/泄密/重复副作用？边界是否易测？是否新增可替代的抽象？

## 7. 执行门与维护债务

本地和 CI 运行 `py -3 tools/code-quality.py`；它检查冻结依赖图、源文件规模棘轮、crate unsafe 禁令和 scope，不取代 Rust 编译器/Clippy/合规门。`py -3 -m unittest tools.tests.test_code_quality` 验证检查器自身的拒绝路径。

每次实现提交仍须 `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、`cargo test --workspace --all-targets`、`bash tools/compliance-doctor.sh`；strict Clippy 与相应永久/产品门按 AGENTS 执行。新增工具不会跳过任何已有门。

`cargo audit`/release audit 继续处理可达漏洞，合规门处理许可证和来源。`cargo deny`/nextest 可以在单独兼容验证后采用，目前不宣称已经接入，也不为引入工具更改冻结测试过滤语义。

已有大模块按风险分批拆分：先 gateway transport/控制面，再 harness orchestration/production，再 store 按事务领域拆。每批保持行为不变并独立提交，避免在安装/认证实现中夹带整库重写。规模基线仅表示已知债务，不表示这些文件已通过可维护性验收。
