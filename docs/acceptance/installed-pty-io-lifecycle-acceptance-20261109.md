# PTY I/O 生命周期修复组件验收（2026-11-09）

实现提交：`451f3b4416e28bbf792d881e052f227cfba5cf23`，分支 `shu/closure-inventory`。本轮落实 PRD08 §4、PRD19 A5 与 architecture05 §4.4 的既有 timeout/cancel/output/evidence 要求，修复普通 `PtyBackend` 的阻塞输入、输出饥饿和收尾等待。公共接口、冻结 DTO、crate 图、99 EventKinds、依赖和原生 unsafe 白名单保持；本报告不构成安装版 PTY 或 IP10/IP11/IP24 验收。

## 1. 旧实现的真实复现

固定自建程序在 Windows build `26200.9711` 上执行。外层独立测试进程持有 kill-on-close Job，8 秒内未结束时只终止原 Job，保存原输出和回执。先行复现为 **3 passed / 2 failed / 1 ignored**：

- 程序不读取 4 MiB 输入时，旧实现先同步写入、随后才启动计时；750 毫秒的配置实际在约 4.48 秒后返回，超过测试的 4 秒收口上限。
- 连续输出且观察者每个增量耗时 50 毫秒时，旧实现持续排空 ready queue，超过外层 8 秒期限，触发原 Job 强制清理。

原 `pty-io-lifecycle-before-fix-native.log`、对应 `outer-receipt.json` 和失败现场保留。先行取消场景不足以证明程序已经进入阻塞点；最终场景改为观察原程序的 PID 标记后才取消，最终完整回归覆盖此断言。

## 2. 实现与恢复边界

读写分别由内部线程处理：读取队列为 16 × 4096 字节，控制回复队列为 4 项，输入按 256 字节片段写入。主执行循环处理一个增量后重新检查取消和期限；设置阶段也计时，Started 接纳后再次检查期限和取消，fallible reader/writer 获取发生在程序启动前。

终端光标查询按增量解析，支持跨片段及与正文合并的查询，保留邻近正文。达到截断预算后继续排空，但停止追加重复截断事件；正常退出仍收集有界尾部。

原 child、master 和全部读写线程由清理线程接管；关闭终端期间继续消费队列，逐一 join 所有线程。调用侧最多等待清理 2 秒。尚未核实的原清理保留监督句柄及原名额，使用既有 `ActionOutcomeUnknown`，并阻断新程序；只允许回收原清理，不重派动作。进程级最多 8 个活动名额，完成但失败的清理保持阻断。

本轮不宣称所有平台初始化、线程资源耗尽或 OS 关闭故障均已验证。2 秒是调用侧的清理等待界限，不是假定 OS 一定已完成清理；实际未完成时必须保留 Unknown。

## 3. 最终八项真实 Windows 组件场景

以下来自最终完整 workspace 的原现场，每项外层测试均正常退出、无强制 Job 清理，并核对唯一 Started、相应终态、CapabilityEvidence 和无 Unknown。时间为该次采样，不是性能保证。

| 场景 | 实测返回 | 终态及断言 |
| --- | --- | --- |
| 输入不读、750 ms 超时 | 1033 ms | ActionFailed；程序实际进入 fixture。 |
| 输入不读、启动后取消 | 390 ms | ActionCancelled；先观察原程序 PID 标记。 |
| 连续输出与慢观察者 | 1876 ms | ActionFailed；超时检查继续获得执行机会。 |
| 严格输出预算耗尽 | 198 ms | ActionFailed；程序实际进入 fixture，原清理完成。 |
| 输出 append 被观察者拒绝 | 183 ms | ActionFailed；拒绝可发生在 ConPTY 初始化输出，未观察到 fixture main 标记，不把它描述成程序正文已运行。 |
| 正常退出及尾部 | 355 ms | ActionCompleted；最终 `FORME-PTY-IO-END` 标记可见。 |
| 截断预算 | 215 ms | ActionCompleted；结果恰为 4096 字节，唯一截断标记且退出可信。 |
| exit 7 | 173 ms | ActionFailed；有失败与能力证据，无完成。 |

ConPTY 输出含终端控制序列，正常输出场景验证尾部标记和预算状态；不以原程序 stdout 的逐字节相等断言代替终端语义。先行错误的逐字节预期及其失败日志保留。八项外层测试各启动一次显式辅助 probe；该辅助入口默认 ignored，不另计为通过，也不把内外两层重复计数。

两个单元场景另验证分片/合并握手，以及保留原监督工作时的新准入拒绝、释放原工作后的准入恢复。后者注入的是受控线程等待，不冒充原生进程关闭故障证据。

## 4. 源码检查及工件绑定

完整组件检查于本地时间 **08:16:12–08:36:04** 完成，fmt、workspace all-targets check/test、workspace strict Clippy、code quality 与完整 compliance 均 exit 0。Rust 为 **187 个测试目标、1,329 passed / 0 failed / 21 默认 ignored**；结构门为 893 files / 63 existing size debts。先行编译、VT 断言和 Clippy 失败保留；最终使用显式循环 join 全部工作，不采用可能提前停止的短路 join。

提交前已独立逐份核对四个源码文件及六份检查日志摘要，并检查本轮八份原现场的期限、终态与唯一性；`pty-io-evidence-verification.json` 为 PASS。检查记录绑定原基底 `9cea970` 加精确四文件快照，实现提交只包含该快照，未以提交后的新源码冒充检查对象。

固定证据区为本工作树 `target/closure-inventory-evidence/`：

| 工件 | SHA-256 |
| --- | --- |
| `pty-io-checks-process.json` | `1f65b4b93ae887bb07037e81967ef75e32ab791216fcfbc4eebe1285f84a181c` |
| `pty-io-evidence-verification.json` | `cd46a5380510389817e5db6ad5ce3bdaba72ab9bbc2890bbcfadc26798e3438b` |
| `pty-io-test.log` | `af07cd7ee75c0bc7dcb5db1c7a3bd5ec91cc41df99233ebfd8d3b1ffd234f177` |
| `pty-io-lifecycle-before-fix-native.log` | `16feed1d82226840b2a8c9c5e71cad646211e3cf7b25919a21a216a5075b9aa2` |
| `pty-io-supervisor-focused.log` | `9e538b06d0e54511b90b6a6dbb883eec2e074e54f950d3be2e688468592fd431` |
| `pty-io-final-build-resources.json` | `9976e810c76887afd1c572b78c310cf09ffdb03a96846458559cba9824aadfd6` |

固定 target 的末次报告 PASS，约 42.9 GiB 逻辑 / 18.7 GiB 分配，未超过 64 GiB 预算。各 Cargo 阶段前的 20 GiB 门均通过；本轮没有删除缓存或降低资源门槛。

## 5. 仍开放的义务

本次八项作用于普通 `PtyBackend`，没有同时证明 Low AppContainer 内相同故障矩阵；[隔离机制验收](installed-isolated-pty-mechanism-acceptance-20261109.md)的原三项结果保持独立。隔离 runner 的初始化/关闭异常矩阵、生产 placement、Host-issued grant、受保护引用输入/输出、overlay 回写与同号恢复仍待闭合。

没有安装、升级、启停服务或更改 Windows UAC，没有刷新完整 Final/M5。三份冻结协议提案及 IP01–IP24 的整体状态继续以[剩余义务审计](installed-product-remaining-obligations-audit-20261109.md)为准；组件 PASS 不替代当前签名安装版的联合证据。
