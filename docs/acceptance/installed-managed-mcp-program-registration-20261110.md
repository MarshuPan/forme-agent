# 受管 MCP 程序注册组件验收

## 范围与源码

实现提交为 `bcd20552eeec6d9bb0b4181571d37b913d0a7235`，位于 `shu/product-integration`。它落实已批准的[注册契约](../architecture/installed-managed-mcp-programs-contract.md)：Host 管理域提供 Register/Revoke、只读目录与幂等操作查询；Gateway 校验 owner、Origin/CSRF、幂等键和绑定完整命令摘要的近期口令重认证。

程序只能来自 Host 本机预置来源，Web 不接收路径、argv、环境或凭据。注册验证只读文件 custody、真实文件身份与摘要；append-only journal 保持单 writer、顺序、hash、fsync、CAS 和撤销 tombstone，独立 binding 原件拒绝日志缺失、替换与错误安装身份。Host 与 registry 共享同一配置实例；请求期限覆盖锁等待和 hashing，维护阻止新 mutation，同义历史回执仍可查询。

该提交未接通实际 MCP 发现/调用、Core metadata 消费或设置 UI，配置启用保护继续拒绝。测试中的 exe 是惰性文件，HTTP peer 是受控 loopback；不构成原生 MCP、安装身份或 IP01–IP24 验收。

## 验证结果

全部证据位于 `target/product-integration-evidence/`，保留原件，不覆盖失败记录。

| 检查 | 结果 |
|---|---|
| 最终 workspace/all-targets | 198 targets；1,494 passed / 0 failed / 22 默认 ignored。 |
| 新增协议、注册、Host 和 Gateway 场景 | 20 项逐名核验通过，包含正常与拒绝场景。 |
| fmt、workspace check、strict Clippy、产品构建 | 全部 exit 0。 |
| 结构质量、完整 compliance | 全部 exit 0；1,016 files、63 项既有规模债务。 |
| 独立证据核验 | 1,046 项源码集合及逐文件 SHA-256、七份日志、计数与资源报告通过。 |
| 提交绑定 | `managed-mcp-program-commit-binding.json` 确认实现提交至 `8fd9ffe` 的被验源码未变。 |

最终测试实际运行于 2026-11-10 05:54:12–06:08:07 UTC；七项检查于 06:09:49 UTC 完成。原 `managed-mcp-program-verified-r2` 的 C14 编排测试失败仍保留：planner-builder-evaluator-integrator 的 node-2 两次 Failed，node-3 Cancelled。原源码单测复跑通过；新增失败事件诊断后，整个编排目标 23/23 与最终整仓均通过。根因尚未确定，不能据此宣称修复或认定为偶发问题。原断言、治理要求和测试范围未放宽。

| 原件 | SHA-256 |
|---|---|
| `managed-mcp-program-verified-r3-gates.json` | `e83793c4c57ef837853828933028e7496689ad3a3f009d145aa43bc3d3bbc46a` |
| r3 test log | `f998b8d92d84bfacd1335aa605ea56005525de09b2a8690f9f545d643b1209c6` |
| source manifest | `b88314a227684d046317fa08d79d7ff936926ee5bd3246880f39a2cf6d658a4f` |
| `managed-mcp-program-independent.json` | `0cdbec33715fc8f6e6e75d1c64e99ba4e1b2ba797fd13b1ff5b6eaa79f730c89` |
| `managed-mcp-program-commit-binding.json` | `a49627e171501a55883653995549e21d21a16447056071b45e94a17fec9f19c9` |
| 原 r2 失败测试日志 | `08b3a5d5e8876388352a91a6a1e1972302901a056c76d4e2e6f6d6f9d89a2890` |
| `managed-mcp-c14-repro-unchanged.log`（UTF-16） | `66443600f25ae43faede745488ea3bfb251eba02c91ec5f9732aee059dd4e46a` |
| `managed-mcp-c-orchestration-diagnostics.log` | `160d42d9b0921019bd305b08a85158acfdb6cc1878ec1b67d43a8b94e4e0674a` |

## 构建缓存维护

完成验证、源码干净且无构建占用时，先预览再对 79 个大型缓存程序执行可逆 NTFS LZX 压缩，逐项核对逻辑长度和 SHA-256。回收 **16,736,690,108 bytes（15.5873 GiB）**；实际分配从 76,785,930,450 降至 **60,049,240,342 bytes（55.93 GiB）**，回到 64 GiB 预算内。逻辑长度 94,635,125,802 bytes、18,479 个文件和 66 个重复硬链接保持不变；未删除缓存、候选或验收证据。

`managed-mcp-cache-compression.json` 的 SHA-256 为 `978f10ea88dde8dbd7aead1113c3c8ee68ab6b6c2788e70cd44d3b16da5d5a65`；后续资源报告 `managed-mcp-cache-after.json` 为 `222678a74fc84ddae2b820c0bb659321319f3cea526970d65f5fa28c4e5238a7`，status=PASS、errors=[]、over_budget=[]。数值是本次采样，不是后续构建的占用保证。

## 剩余工作

`8fd9ffeab207abda42a3dd209bf4a0b2c04f78e8` 已提交[stdio 隔离与实际出站契约](../architecture/installed-managed-mcp-stdio-contract.md)。后续须实现原 AppContainer/Job 内的程序根进程、三个明确 pipe、独立取消监督、实际写入前的配置/注册重查、完整 I/O 收束、Host 结果封存、Core 受保护 metadata 与认证 owner 发现入口。

本组件尚未重跑 M5、D、完整 Final 或 IP，不沿用旧提交的回执证明新源码。插件、渠道、后续 profile、桌面联合验收和完整安装目标仍开放。本批未部署，未修改系统服务或 UAC；原桌面候选未重建或覆盖。
