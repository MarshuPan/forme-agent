# 受管 MCP 普通调用组件验收（2026-11-10）

实现提交 `c0a0bde7e446a02a0beb5d592510a20af07775b7`。本批落实已批准的 [stdio 契约 §4](../architecture/installed-managed-mcp-stdio-contract.md)：普通工具调用只消费原已完成发现，经正式隔离 worker 执行一次。Core metadata 持久化与恢复、owner 显式发现入口、Web 设置和安装版联合验收仍在后续范围；公开 enabled 保护保持。

## 实现与恢复

- 原 Worker journal 的私有 execution binding 对新发现保存有界的完整原请求（独立版本 2，最多 16 KiB）；普通动作及历史版本 1 保持原字节形状。旧发现可以按原号核实，但缺原请求不能作为工具可用来源。未改变 WorkerJob/Permit/ExecutionSpec、旧 Mcp 字段、99 EventKinds、18 crates 或五个 unsafe 边界。
- Host 按 journal admission 顺序选择最新发现；pending、failed、Unknown 或损坏的新来源阻断旧来源回退。独立核验 Completed receipt、manifest、AEAD 原件、catalog/schema、当前 settings/program/scope/allowlist；不是只信任主脑缓存。
- 普通调用仅接受 opaque program_ref 和空 args/env。固定 initialize → notifications/initialized → tools/call，不执行 tools/list/resources/list。每次实际 pipe 写入重新核对配置、注册及同一发现原件；共享原 deadline 和总输出预算。
- 尝试写 tools/call 前记录可能已派发；部分写入、调用后取消、错误帧、额外输出、非零退出或失联均保留 Unknown，重放只查原号。派发前已确认失败才可 Failed/Cancelled；已收到可信结果后撤销不抹掉该历史结果。
- 成功/明确工具错误均经原树退出、EOF、reader join、Host AEAD 封存和清理。WorkerActionBackend 沿原 McpCallEvent 记录普通调用，发现不伪造工具调用事实。原 worker 只读查询拆入职责模块，既有规模债务由 63 降到 62，未调整基线。

## 最终源码与原生证据

2026-11-10 16:13–16:31（Asia/Shanghai）九阶段检查全部 exit 0：fmt、workspace/all-targets check、strict Clippy、完整 tests、产品程序构建、原文件结果、原 PTY、结构质量与完整合规。整仓 **199 targets、1,509 passed / 0 failed / 32 默认 ignored**；新增六项常规测试均逐名核验。

同一源码在完整检查后另行执行原生 MCP 矩阵 **10/10 PASS**（25.10 秒，六项新增调用场景与四项发现回归），没有把默认 ignored 计作 workspace 通过。正常调用/Host 重启验证请求序列无重复发现、只有一次 tools/call；缺原件/schema/settings 漂移、实际写入竞争、明确工具错误与 Unknown、调用后取消/阻塞写、新 pending 来源遮蔽旧来源均取得原生组件证据。先前 10/10 原件保留。

原文件结果封存与未封存保留 **1/1 PASS**；原 PTY **4/4 PASS**，对应 11 份独立实验回执，result/cleanup 均 PASS、原 Job 空且 installed_acceptance=NOT_RUN。最终 1,067 项源码、九份日志、MCP 最终原生日志、执行二进制/fixture 与 PTY 原件已逐项独立核验，提交后源码再次匹配；manifest SHA-256 为 `e9261adbe41cf65e8922351270b51fdad9bf4fd15a6a4a5d626dbe951c2dcd7e`。质量门为 1,037 files、62 项规模债务。

## 保留的失败与修正

首次原生矩阵 0/9：fixture 预期的记录文件缺失，随后隔离清理未确认阻断后续准入；原失败日志保留，不将失败场景算作通过。第二次 8/9，重启场景在 Host writer 仍持锁时直接读取 journal，改为释放该 writer 后再独立读取。另一次 strict Clippy 指出会话枚举尺寸差，改为装箱 discovery 状态；最终整仓和原生复核覆盖修正源码。相关 `managed-call-r1-native.log`、`managed-call-r2-native.log`、`managed-call-r3-clippy.log` 未覆盖。

## 原件与空间

原件在本工作树 ignored `target/product-integration-evidence/`，不存在新时间戳 target。沿根 dev/test 节省空间参数、固定 target 和两路 Cargo 编译。检查结束时实际分配 **75,716,264,906 bytes（约 70.52 GiB）**，超过 64 GiB 报告阈值；卷剩余空间门通过。后续空闲维护另存精确回执，不以预算告警自动删除缓存或验收证据。

| 原件 | SHA-256 |
| --- | --- |
| `managed-mcp-call-r1-gates.json` | `4321a792acb0f3415667f3cd8a95d251d0804314960f534107ce6640da129bc4` |
| `managed-mcp-call-r1-independent.json` | `06cda7f616b9eb5301ef98629a3cd64574212601ba811ee7b3dbbb3016c50f83` |
| `managed-mcp-call-r1-commit-binding.json` | `45abd192309d8f6f38acfa7868e3d248c066286157656dc0b269c1f6ca0d253e` |
| `managed-call-final-native.log` | `638377b2c707c2560caa63904b73e8ef560089b9fdfdb03d7da0434a494d3759` |
| `managed-mcp-call-r1-native-existing-results.log` | `82a7c80ca5d4869daaac7c3c65de3303ea75ef644090e07574438f991ff60ec3` |
| `managed-mcp-call-r1-native-existing-pty.log` | `bf4d0f12acac563efe942082c19289eae233a083c4323cf6f39f13c600422093` |
| `managed-mcp-call-r1-build-resources.json` | `648ea19b7adeaab0101d6518296eeb2dda3bc375f53b4751269ed4a1df973683` |

## 仍未关闭

Core 必须保护并消费原 metadata、处理删除与重启恢复；owner 发起发现需进入原 Harness exact-plan 审批，Web 通过正式认证接口接入。启用保护需等全链路验证后才放开。插件、渠道、后续 MCP profile、桌面联合验收及完整 IP01–IP24 义务保持。本批没有部署、推送或系统服务/UAC 操作，没有覆盖已有签名安装候选，也未将组件证据提升为当前 M5/D/Final/IP。
