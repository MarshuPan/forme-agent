# 旧脑首次认领：journal v2 组件验收（2026-11-04）

本报告只覆盖已经批准的[旧脑与备份候选协议](../architecture/proposals/installed-bootstrap-legacy-brain-claim.md)中的持久认领状态和只读查询。协议、候选根与 DTO 已先进入设计基线，本批不增加 crate、依赖边、脑 EventKind 或 unsafe 边界。

**完整旧脑接管仍未完成，IP13/IP24 仍开放。** 本批测试中的候选与 worker 回执均为原创隔离 fixture，直接交给私有 journal；它们不是实际历史库发现、解密、迁移或本机激活的证据。

## 1. 已实现的部件

- claim journal 读取完整 schema 1 前缀，保留原字节及摘要链；首个 v2 fact 后使用 schema 2 与独立摘要域，禁止降级。未知类型/版本、截断、摘要或状态不一致均拒绝，原文件保留。
- 追加 `CandidateInventoryObserved`、`CandidateSelectedV2`、`ClaimDataPrepared`、`ClaimCommittedV2`、`ClaimActivatedV2`、`ClaimAbortedV2`，每次先校验纯 fold，再写入并同步，成功后更新内存投影。写入不明后本实例拒绝继续写。
- 每个原 operation 保留 catalog、epoch、候选、owner source/ref/binding、数据准备、commit、activation 与 abort。绑定不符和非 available 候选零追加。
- `Committed` 与 `Activated` 分开。没有激活证明的准备/提交在重启后显示 `RecoveryRequired`，只读查询不执行 worker、服务或配置切换，不凭旧回执自动推进。
- 未准备数据前才允许保存 abort；准备/提交后的 abort 被拒绝。v2 选择未结束时，旧 v1 新脑选择与本机证明轮换不能覆盖它。
- `BootstrapAuthority::claim_state_v2` 核对内部 service 身份和 exact setup binding，返回经过 closed DTO 校验的状态，不开放 v2 数据 mutation HTTP 入口。
- 永久宿主门逐名执行新增的十项 journal 场景，每项必须恰好运行一次；测试消失、被忽略或零执行均拒绝。

## 2. 验证记录

| 层次 | 命令/范围 | 当前结果 |
|---|---|---|
| 定向组件 | `cargo test -p forme-execution --lib bootstrap::` | 22 passed / 0 failed / 0 ignored；包含 12 项既有 bootstrap 场景和 10 项新增 journal 场景。 |
| 生产与测试编译 | `cargo check --workspace --all-targets` | PASS。 |
| 定向严格静态检查 | execution 的 `cargo clippy --all-targets -- -D warnings` | 初次 21 项版本 PASS；追加 hash-valid 降级反例后的完整版本待最终门收取。 |
| 规模与边界 | `py -3 tools/code-quality.py` | PASS：803 files，63 项既有规模债务，未增加债务。 |
| 全量 Rust / 合规 / 宿主命名门 | workspace tests、完整 compliance doctor、`tools/verify-installed-host.ps1` | 正在复验；以本报告后续写入的最终结果为准。 |
| 本机接管、原生服务与完整产品 | 正式包、真实候选、key resolver、独占检查、浏览器和 activation | NOT_RUN；不由本次授权或组件门推定完成。 |

反例不仅检查损坏摘要：另构造摘要与前向 hash chain 都正确的 v1 record，证明进入 v2 后不能通过一条有效旧格式事实降级 reader/writer。

Windows 的独占文件锁会拒绝测试通过第二个句柄读取 journal。测试用 `cfg(test)` helper 经原持锁 writer 句柄快照并恢复位置，未放宽生产锁或重新打开第二 writer。

## 3. 尚未闭合的依赖

1. 从安装器固定历史目录/备份目录发现真实候选，以独占、只读、key、owner/store/schema、保护状态、完整性与 replay 结果构造安全 catalog。
2. 正式 broker 的重新验证、选择、数据准备与 claim commit；不能把浏览器或 worker 自报的已验证布尔值当事实。
3. 在数据 worker 开始前持久化 intent，并把未取得 `ClaimDataPrepared` 的未知副作用纳入同一原 operation 恢复。当前 journal 部件不证明这个窗口已经闭合。
4. Web setup 的 v2 持久候选绑定、账号承诺事务、签名结果和断线后的原号查询。
5. 真实新目标、active data/config CAS、authority fence、ack/post-switch probe 与 `ClaimActivatedV2` 的联合事务。
6. 正式安装包与真实 OS 权限/锁/key resolver/浏览器联合 IP13/IP24；已认领安装不能通过 bootstrap 换 owner。

本机原中断升级仍是另一条未决操作。正常 `AbortBeforeSwitch` 必须继续拒绝缺少升级前基线的旧操作；[独立历史恢复审查方案](../architecture/proposals/installed-legacy-release-recovery-review.md)已另获协议细化与隔离实现批准，仍不授权本机程序替换、journal 迁移、服务启停或重发原升级。它不是本报告 journal 组件的验收内容。

## 4. 后续完整源码门重验（2026-11-05）

在包含 `8991875` 的当前源码上，`tools/verify-code-quality.ps1` 已以 exit 0 完整通过：180 个 Rust targets、1168 passed / 0 failed / 15 默认 ignored，fmt/check/strict Clippy、21 项结构/缓存工具回归、810 文件规模门（63 项债务未增长）、前端/打包/临时 NTFS/只读防火墙策略及完整默认合规均通过。这里的总数是整个工作区，不是 bootstrap 新增测试数量。

`tools/verify-installed-host.ps1` 同轮 exit 0、266/266 个命名场景通过，十项 claim journal v2 场景全部逐名恰好执行一次；上文 §2 当时“正在复验”的完整 Rust/合规/宿主门与新增降级反例的 strict Clippy 义务由本条收取，原时点结果不改写。完整日志和相应核心/M5 回归入口见[本轮组件报告](installed-legacy-release-review-20261105.md)。

该更新仍不证明真实旧脑发现/数据复制/账号事务/激活或 IP13/IP24。默认忽略的原生场景和未提供编译器的 Inno 退出夹具均未计为通过，没有改动安装现场。
