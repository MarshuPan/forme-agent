# PTY 启动前事件与取消边界修复验收（2026-11-09）

实现提交：`0e6dccfc3bbddc234ac0f45a0d1d9b4b02b93974`，分支 `shu/closure-inventory`。这是既有 M2 `PtyBackend` 的组件治理修复，承接 PRD08 §9–10、PRD19 A5 和 architecture05 §4.4；不是安装版 PTY、完整 Final 或 IP11/IP24 通过声明。

## 1. 实际问题与修复

原实现先调用 `spawn_command`，取得程序与终端句柄后才发 `ActionStarted`，并且只在此后检查取消。因此启动事件追加被拒绝、追加期间被取消以及调用前已取消时，程序仍可能已被创建。

新增自建夹具在进入交互前独占创建本次临时 marker。初版只查文件效应，实际仅复现了预先取消仍产生 Started：1 failed / 3 passed。Windows ConPTY 可以在 Rust main 之前暂停程序等待终端查询，所以“没有 marker”不足以证明零启动。强化检查通过只读 CIM 查询，将固定夹具 image 与唯一临时 marker 命令行关联；旧实现实际为 **1 passed / 3 failed**，其中两项明确观察到接纳前已有夹具进程。两份原日志均保留，不将初版假阴性隐去。

现在在申请终端前检查预先取消；在创建程序前接纳 `ActionStarted`，接纳后再检查一次取消。拒绝追加时不启动程序，取消在该切点到达时返回既有 Cancelled。创建期间到达的取消继续沿原 child 收尾；不增加新的取消或完成类型。这里的“零启动”指原 PTY 用户程序，不把分配终端资源说成程序执行。

最终夹具 marker 使用 create-new，正常场景核对原字节与真实终端往返。修改范围只有 `execution/src/pty.rs`、既有 PTY fixture 与独立 `pty_admission_contract.rs`；18-crate 图、99 EventKinds、冻结 DTO/trait 和五个原生边界不变。受管产品也没有回退到这个进程内 driver。

## 2. 验证结果

| 检查 | 实际结果及范围 |
| --- | --- |
| 修复前进程反例 | 4 项中 3 failed / 1 passed；原程序在 Started 接纳前存在，或预先取消仍进入 Started。 |
| 修复后专项 | 新增 4 项与原 M2 backend 4 项共 8 passed / 0 failed；保留正常 PTY、环境、secret redaction、超时、取消和严格输出预算场景。 |
| 最终 workspace | **185 个测试目标，1,317 passed / 0 failed / 16 默认 ignored**。包含最终 create-new 夹具和全部四项新场景；ignored 不计通过。 |
| 格式与编译 | `cargo fmt --all -- --check`、`cargo check --workspace --all-targets` 均 exit 0。 |
| Strict Clippy | `cargo clippy --workspace --all-targets -- -D warnings` exit 0。 |
| 结构质量 | PASS：887 files、63 existing size debts；不表示历史规模债务已清零。 |
| 完整合规 | `bash tools/compliance-doctor.sh`：upstream/license/copy 全部 PASS。 |

专项的 fixture 随后加强了 marker 独占创建和失败证据断言；最终版由完整 workspace 覆盖。专项日志与最终源码回执分别保存，不混称为同一 source snapshot。

## 3. 进程与空间回执

固定证据目录：本工作树 `target/closure-inventory-evidence/`。原验证进程在本地 06:33:39–06:51:35 完成 fmt/check/test 后，被 **8.9 GiB** 的空间门阻止启动 Clippy，原进程回执仍为 exit 1。不能把它改写成整门成功。

随后确认所有 Cargo/rustc 与所选主缓存程序均无占用，针对 `C:\code\forme\target\debug\deps` 的 **93 个**闲置 exe 做可逆 NTFS 压缩。每份内容摘要、逻辑长度和普通文件属性均核对；主工作树 HEAD/dirty state 保持，没有删除文件或更改目录压缩继承。可用空间在该窗口从约 8.95 GiB 恢复至 24.15 GiB。当前工作树缓存的逻辑/分配大小约为 42.18/18.49 GB，不能用其约 30 MB 的增长解释先前整卷空间下降。

续跑先核对原进程的三项成功结果、日志摘要、同一 base 与三个源码摘要，仅运行未执行的 Clippy/quality/compliance，**exit 0**。不重跑已经完成的 workspace，也不降低 20 GiB 阈值。当前固定缓存报告为 PASS、无超预算项。

三个最终源码 SHA-256：

| 文件 | SHA-256 |
| --- | --- |
| `crates/execution/src/pty.rs` | `c76473ea030e52eb58d149f6cae2ba5d06b28a2a458919079edd052838faf884` |
| `crates/execution/tests/fixtures/pty_fixture.rs` | `ca8afcfc12426574d583ca5dde104fba764075eeb5329dd32e843fc962ffa5d4` |
| `crates/execution/tests/pty_admission_contract.rs` | `25b7ecf785e40dcec6c6bdad8e8351c820b684b636a587a7d92526268274f7d1` |

原始验证工件：

| 本地工件 | SHA-256 |
| --- | --- |
| `pty-admission-process-observation-before-fix.log` | `924dc832e891aa0ca93d51d6843e192704e98ed2c301c5e5d68de00253a57c95` |
| `pty-admission-focused.log` | `58704fd84a036627b4c79a9aadfebbca398c7efe34746f4d40eae8774ff2ecb2` |
| `pty-admission-checks-process.json` | `ea8b926ec628bce4760e98e753cca67781520859acb87d1e1b0458d1ae39a6e6` |
| `pty-admission-remaining-checks-process.json` | `1e3c6eb3e838528a903980d420baa34c1dda4d354aa8d6d42b70a35f76194c3d` |
| `pty-admission-primary-cache-compression.json` | `9338f1907048cd732ae67f9761213650d38f2c8e0812073aafd286db251b98c6` |
| `pty-admission-build-resources.json` | `2a121f0a52b949c2a87f8c0c419d7b2dbc490f4e7e89c6a6d8cdaa784c45ad8c` |
| `pty-admission-native-wait-observation.json` | `da75a7e46854dd6ffdf5dc3cf87fdfa73e8948a2e47522bce5ebb1515b2824bd` |

各阶段日志与摘要由两个 process JSON 关联；原先的初版 `pty-admission-before-fix.log` 也保持原件。

## 4. 真实安装与整体目标的边界

本地 06:57:48 的独立观察仍为 Administrator=false；原 `pwsh` PID 45628 与 `consent` PID 65840 存活，三个原管理员启动/native/product 窗口回执仍缺失。Host/Web/Brain 分别以原 SCM 身份 Running，updater Stopped；这不能证明新升级或当前源码已部署。本轮未改 UAC、原封存批次、程序、配置、服务或历史 Unknown，也未重发原请求。

仍开放的具体工作：

- 原三份受管消费者、桌面授权和截图结果提案等待明确契约确认，不将问题送达的 `accepted:true` 当批准。
- 真实隔离 PTY 需要另取 AppContainer、Job、后代、文件/网络/环境及退出证据，机制范围见[验证设计稿](../architecture/proposals/installed-isolated-pty-mechanism-spike.md)。该稿尚无执行结果，不扩大生产 file profile。
- 现有 PTY 输出持续排空、阻塞输入及终端关闭等待仍需有界故障注入；本次只修复启动接纳，不能据此宣称整个 I/O 生命周期已安全闭合。
- 引用输入、二进制结果和其余安装版消费者、原生升级/恢复及 IP01–IP24 的缺口保留；完整 Final/M5 的历史 `6899f56` 回执没有被提升为本次源码回执。

整体目标继续进行，未完成、未暂停。本次完成的是一个已实际复现并修正的执行治理缺口。
