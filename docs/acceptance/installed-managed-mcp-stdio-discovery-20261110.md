# 受管 MCP 原生发现执行组件验收（2026-11-10）

实现提交 `587d0ada693390032beeb6e48e6b5cd408501ef6`。本批落实已批准的 [stdio 隔离契约](../architecture/installed-managed-mcp-stdio-contract.md) 中发现会话部分：注册的程序成为 Low AppContainer 根进程，经原 Worker journal、Job、取消及 Host 加密结果通道完成 discovery。普通 tools/call、Core metadata、owner 发现入口和安装版联合验收仍未完成。

## 实现与治理

- Host 与 worker 复用同一个程序注册权威。配置锁覆盖原 journal start 与原生准入；每次实际 pipe 写入重新核对 owner/epoch、完整 settings、scope 和原注册。获取锁可被原期限及取消打断，独立 Job 监督不依赖配置、worker 或 registry 锁。
- 程序直接处于低完整性、空网络 capabilities 和禁止子进程的 AppContainer；仅继承明确的三个 stdio pipe。环境只有固定系统目录及隔离目录所需五项，独立空 workspace 不映射用户文件。注册程序及祖先的文件 custody 保持到树、I/O、ACL 清理完成。
- 固定握手与声明过的工具/资源列表，允许有界分页；错 id、重复/主动/非法/不完整帧、超量、尾部和非零退出均不发布部分 catalog。stdout/stderr 累计共用预算，所有阶段使用原单调 deadline。
- 关闭 stdin 后，只有根进程正常退出、原 Job 空、两个流结束及读线程回收全部确认才产出结果；原 Host AEAD 封存后才撤销 lease、移除原目录并登记回执。未确认清理保留原资源及 Unknown，不能换号重派。
- Win32 操作仍局限在原五个审核边界，原生主文件为 600 行。18 crates、99 EventKinds、冻结 Worker trait/DTO 不变；新增原创 fixture 不纳入安装包。

## 最终源码验证

2026-11-10 14:49–15:09（Asia/Shanghai）的十阶段门全部 exit 0：fmt、workspace/all-targets check、strict Clippy、完整 tests、产品程序构建、新 MCP 原生场景、原文件结果场景、原 PTY 场景、结构质量、完整合规。

完整 workspace 为 **199 targets、1,503 passed / 0 failed / 26 默认 ignored**。新增 7 项交换状态测试与 2 项锁等待测试逐名通过；C14 八模式编排在本次完整回归通过，没有用旧的单测结果替代。结构质量为 1,027 files、63 项既有规模债务；未更新基线。

固定源码全集的 **1,057 项** SHA-256、十份日志、执行的原生测试程序、fixture、11 份原 PTY 原生回执和最终计数已另行核验，提交后再次逐文件确认一致。源码 manifest 摘要为 `5503c1fd83d096319fec15da314cb1143082bca5a515702a882b64361fd48a53`。这不是安装产品或跨平台证据。

## 显式原生实验

1. 新 MCP 原生 **4/4**（最终运行 12.08 秒）：正常分页发现及真实 Host 加密封存；八种异常输出拒绝；撤销/禁用/配置锁占用时取消；原 1.5 秒期限终止失联程序。
2. 正常请求序列精确为 initialize → notifications/initialized → tools/list → tools/list → resources/list。原生 fixture 验证私有文件、sources.json、Host 环境标记、子进程和测试 loopback 连接均被拒绝。加密结果具有 FRMRES01 头且无明文工具名；正式 manifest/chunk 读取后核对正文/catalog 摘要，同号 dispatch 取原回执，原 MCP 目录已清理。
3. malformed、wrong-id、duplicate、incomplete、oversized、stderr、tail、nonzero 均 Failed 且无部分 catalog。stderr 单独 131,000 bytes 未超 131,072 预算，加上 stdout 后拒绝，证明累计计数。撤销/禁用/锁竞争实验只发送 initialize，零后续 RPC；独立 OS 观察核对实际根进程及原 AppContainer/Job。
4. 原文件结果封存与未封存保留 **1/1**；原 PTY **4/4**，对应 11 个独立实验，包括八项 I/O、生产派生限制、终端往返及后代取消。11 份回执均 result/cleanup PASS、原 Job 空，且 installed_acceptance=NOT_RUN。

MCP settings 由 cfg(test) 专用 fixture 写入真实配置 journal，以测试隔离组件；公开保存的 enabled 保护未放开。没有借这条测试入口声称 owner/Harness/Web 或已安装服务端到端通过。固定请求较小，本批也不扩大为任意大小工具输入的阻塞写入证明。

## 原始失败与修正

先行编译暴露 owner principal 类型转换、测试身份认证和结果状态类型的错误；Clippy 暴露两处简单写法问题，均已修正。首次正常原生实验误把注册 CAS revision 固定为 0，改为读取真实 catalog revision；首次四场景矩阵 3/4 通过，最后一项错误要求默认不可用的文件 adapter 提供全 Host quiescence，改为核对本 MCP 的原目录清理，未削弱生产守卫。相关 r1/r3 check、Clippy、normal-r1、matrix-r1 原件保留；后续最终整门覆盖修正源码。

## 原件与缓存

原件在本工作树 ignored `target/product-integration-evidence/`；旧原件未覆盖。日志和二进制逐项摘要在独立核验 JSON，主要索引如下：

| 原件 | SHA-256 |
| --- | --- |
| `managed-mcp-stdio-r1-gates.json` | `b0cc703145da9a18e4a57870dedf1e2bdb19b233ec8c75b56e5d824cae4027bf` |
| `managed-mcp-stdio-r1-independent.json` | `54df6a2f9df56501cf475473371a48ac1ee62af3f741ddf1b178be156b38a077` |
| `managed-mcp-stdio-r1-commit-binding.json` | `22accb2609701c6fed6c8275ec47b4a668ef5530f13c08ad11d7ae73929d3721` |
| `managed-mcp-stdio-r1-test.log` | `96c00eb9c97e7a17a661cb650247529538b4c7e506fdef8b433296dae1b00590` |
| `managed-mcp-stdio-r1-native-stdio.log` | `7ce082ace361c78a2f7f5b642024749919d9e806dd84a498103a938d06513206` |
| `managed-mcp-stdio-r1-native-existing-results.log` | `a6449c18cdb6d8d45000108a43e814a76949e73281be38180215481b10eff5dd` |
| `managed-mcp-stdio-r1-native-existing-pty.log` | `e87d95063693caeb5730c40b912fc4b68a015ca419f6e5135cee053a8f5be8f5` |
| `managed-mcp-stdio-r1-build-resources.json` | `788ffedfd23158d08f0561b17176fc4e34a020186ed9791c13fe4a86ff362b68` |
| `managed-mcp-stdio-cache-compression.json` | `6e5968fe8ed5386c260584a460efb86dd3079a5057e2ac0bf033c889f6069ec7` |
| `managed-mcp-stdio-cache-after.json` | `eb037e6b85d78d2716f4c1eaaa9300b5417e3df86a3ebd2ebd665c44bca4dfbc` |

本轮构建后缓存实际分配为 75,039,520,665 bytes（约 69.89 GiB）。源码已提交且确认无构建/测试进程占用后，先预览，再对 72 个大型缓存程序执行可逆 NTFS 压缩，逐项长度与 SHA-256 保持，回收 **15,046,605,875 bytes（14.01 GiB）**。后续空间报告实际分配 **59,992,914,790 bytes（55.87 GiB）**；这只是该时点采样。未删除缓存、候选、源码或验收证据，继续复用固定 target 和两路 Cargo 构建。

## 未关闭的义务

接下来需要 Host 核对原 completed discovery/catalog 后执行普通 tools/call、Core 受保护 metadata 与删除/重启恢复、owner 显式发现和原 Harness 审批、Web 设置入口。启用保护在正式链接通前保持拒绝。插件、渠道、后续 MCP profile、桌面联合验收和完整 IP01–IP24 继续按原范围实施。

本组件尚未刷新 M5/D/Final/IP，永久门后续结果另行记录。本批未部署、未推送、未操作系统服务或 UAC，未重建或覆盖旧桌面签名安装候选。
