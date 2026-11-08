# 安装版 worker 文本结果类型纠正（2026-11-09）

状态：`cff8a29` 的源码修复与组件回归通过，尚未部署；不是完整 Final、签名升级或 IP01–IP24 通过。基线为 `fb00695e31a8bfe4f4479eda2c2d8ff206f673f4`，检查在该基线加本次两份源码差异上执行，提交前逐份核对同一源码及检查日志摘要。

## 1. 问题与修复

现有 `WorkerActionBackend` 将核验后的 worker 结果投影为文本。此前它验证 manifest/chunk 的绑定、长度和摘要，并检查正文能否按 UTF-8 解码，却没有限制 manifest 的声明类型。合法的 Binary manifest 若承载恰好可解码的正文，会被投影为文本并产生完成事件。

在生产代码未改动时，新回归的首个 `BinaryUtf8` 分支实际失败：返回 `Completed`，预期为 `Unknown`。测试当时在首个断言中止，不声称另外两个分支也在修复前执行；原终端观察转录保存在本工作树 `target/closure-inventory-evidence/worker-encoding-before-fix-observation.md`。

修复在完整 manifest/原 job 绑定检查后、读取正文前，要求现有文本消费者只接收 `WorkerResultEncoding::Utf8`。Binary 返回原动作的 Unknown，不输出正文、不产生完成或成功能力证据；不改写 Host 的原终态。普通文本、空文本与跨块 UTF-8 沿原有核验路径处理。

这沿用 PRD24 §4.1.1a 的结果核验与 Unknown 边界，不改变 public trait、协议对象、序列化、99 个 EventKinds、18-crate 图或原生代码边界。通用 worker 协议仍可描述 Binary；截图的正式结果消费者继续按待批[桌面结果接线提案](../architecture/proposals/installed-desktop-output-closure.md)实施，不借本修复启用截图或其他能力。

## 2. 验证与原始证据

| 范围 | 实际结果 |
| --- | --- |
| 修复前新增回归 | 实际 exit 1，`BinaryUtf8` 被错误完成。 |
| 修复后 worker backend 专项 | 12 passed、0 failed。 |
| 工作区 `cargo test --workspace --all-targets` | 184 targets，1,313 passed、0 failed、16 默认 ignored；ignored 不计通过。 |
| 格式、整仓 check、strict Clippy | 全部 exit 0。 |
| 结构质量与完整 compliance | 全部 exit 0；886 个文件、63 项既有规模债务，基线未改。 |

新增场景覆盖非空 UTF-8 字节声明为 Binary、空 Binary 和非 UTF-8 Binary。每种 manifest 自身通过 schema/digest 校验；重复核实后仍为 Unknown，dispatch 总数为 1、probe 为 1、正文读取为 0，且没有替代取消或输出/完成/成功证据。已有合法 UTF-8、空结果、跨块、错误正文、摘要/绑定篡改、取消及丢响应恢复回归继续运行。这是受控 transport 的组件证据，不是原生 worker 正向执行证据。

本工作树的机器回执与日志：

- `target/closure-inventory-evidence/worker-encoding-focused-completion.log`：专项末次真实输出。
- `target/closure-inventory-evidence/worker-encoding-checks-process.json` 与 `worker-encoding-{fmt,check,test}.log`：已完成的前三阶段；原总退出码 1 保留。
- `target/closure-inventory-evidence/worker-encoding-remaining-checks-process.json` 与 `worker-encoding-{clippy,quality,compliance}.log`：仅续跑剩余三阶段，实际 exit 0。
- `target/closure-inventory-evidence/worker-encoding-build-resources.json`：本工作树缓存报告，PASS。

原流程在完整测试通过后，被下一阶段的 20 GiB 空间门以约 9.3 GiB 可用空间阻断；没有重跑前三阶段。盘点后、维护执行前，可用空间已回升到 24.97 GiB，`worker-encoding-primary-cache-compression.json` 记录实际压缩文件数为 0、目录未修改、主仓库 Git 状态保持。没有通过删除文件、放宽阈值或改写旧回执续跑检查，也不归因未经验证的空间变化原因。

两份已验证源码的 SHA-256：

| 文件 | SHA-256 |
| --- | --- |
| `crates/execution/src/installed/worker_backend.rs` | `d5d266e9a58f319d7682cde92974e7c5d65505769829ff6c9677c87137e3313f` |
| `crates/execution/tests/installed_worker_backend_contract.rs` | `94697222f5ed03ec5beb05da6e724b61aec572ed5ef47423b965de6bbe62668c` |

## 3. 安装与整体目标的边界

本轮只读核对 SCM：Host/Web/Brain 为 Running，updater 为 Stopped。原提权 launcher 与 consent 进程仍在，原 `correction-scope-elevation-launch-20261109.json`、`correction-scope-native-window-20261109.json`、`correction-scope-product-window-20261109.json` 仍缺失；没有新起提权、修改 UAC、启停服务或重派原 Unknown。

原生升级仍沿原批次取得实际结果。受管消费者、桌面授权、桌面结果三份冻结提案继续等待明确确认；引用输入、真实 PTY 与其他安装版 placement 的实施和平台证据仍开放，详见[剩余义务审计](installed-product-remaining-obligations-audit-20261109.md)。本修复不缩小整体目标，也不将正确的 Unknown/unavailable 当作能力完成。
