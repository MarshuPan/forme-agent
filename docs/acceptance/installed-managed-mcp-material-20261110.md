# 受管 MCP 的 Core 原件保护验收（2026-11-10）

实现提交 `f3755062971523b0a0d6a0b873cc6a8e38672c5d`，承接已批准的 [stdio 契约 §6](../architecture/installed-managed-mcp-stdio-contract.md)。发现结果必须先保存为可独立核验的加密原件，再产生 ActionCompleted；本批不发布 registry metadata，不开放公开 enabled 保护，也不代表安装版 MCP 或 IP01–IP24 完成。

## 实现

WorkerActionBackend 使用独立的 Core discovery consumer，不改变既有 ArtifactStore 截图接口、Worker wire DTO、EventKind、crate 图或 unsafe 边界。缺 consumer 在计划及执行前拒绝；consumer 核对原 request、Completed observation、manifest、完整正文和 catalog，使用原 job/permit/manifest 派生确定性内容地址。

Core 在同一 StoreContentProtection/lifecycle 中保存加密正文、独立版本化 companion 和既有 material receipt；三者均能完整重读后才返回内容引用。相同原件只补齐中断写入，重放不会创建新 job 或密钥；原号查询可以更新观察时间，不能替换终态和原绑定。冲突、owner 替换、正文/摘要替换以及 pending/完成删除 fence 均拒绝。脑输出仅含有界摘要和受保护引用，目录正文仍是 UntrustedData。

consumer 失败或返回伪造引用使原动作保持 Unknown；完成事件持久化失败直接返回错误，不追加矛盾 Unknown。成功重放只核实原 worker，不重新发现。生产装配已接上 consumer；后续 registry 仍须独立核验原 run 的完整事件链及当前配置/来源。

## 验证

2026-11-10 16:43–17:04（Asia/Shanghai），fmt、workspace/all-targets check、strict Clippy、完整测试、产品构建、原生 MCP、原结果、原 PTY、质量和完整合规共十项全部 exit 0。整仓 **199 targets、1,517 passed / 0 failed / 32 默认 ignored**；新增八项测试逐名通过，覆盖加密和重启幂等、三处写入中断、冲突与身份替换、部分/完整删除、保存先于完成、失败及替换引用、完成 append 失败。

这些新增场景使用真实 SQLite、加密和密钥服务，以及受控 WorkerBrokerTransport；不是安装身份的 Core 联合验收。另行显式执行的原生 MCP **10/10**（24.58 秒）、原结果封存 **1/1**、PTY **4/4** 通过；PTY 的 11 份独立回执均为 result/cleanup PASS、原 Job 空、installed_acceptance=NOT_RUN。默认 ignored 没有计入 workspace 通过项。

独立检查核对了 1,072 份源码、十份日志、原生二进制/fixture 和 PTY 回执；提交后再次核验源码一致。源码 manifest SHA-256：`4a1eb3c974bddb8181348787eeeacc89b13e12d077ac7563499494bb622ae5f2`。质量门为 1,042 files、62 项既有规模债务，未刷新基线，原超限 production.rs 行数保持。

开发检查期间发现的测试导入、catalog digest 方法名、Source 枚举和未使用导入问题已修正；原失败日志保留于 `core-mcp-material-*-check.log`。最终完整门使用修正后的同一源码。

## 空间维护与原件

继续使用固定 target、根节省空间 profile 和两路 Cargo 编译。全部进程退出且源码提交干净后，先预览再对 73 个大体积 exe 缓存做 NTFS LZX 可逆压缩；每个文件压缩前后 SHA-256 相同，Git HEAD/status 保持。没有删除缓存、安装包或验收证据。

实际分配从 **75,736,343,771 bytes（70.54 GiB）** 降至 **60,064,288,564 bytes（55.94 GiB）**，回收 **15,672,055,207 bytes（14.60 GiB）**。最终空间报告 PASS、无错误，低于 64 GiB 预算。

以下原件位于本工作树 ignored `target/product-integration-evidence/`：

| 原件 | SHA-256 |
| --- | --- |
| `core-mcp-material-final-gates.json` | `28fa57b511d0247047de523369091cf1cc0f2388f372aa22478c99a164bed3a5` |
| `core-mcp-material-final-independent.json` | `8acf06af4aa734dd3edbb37c365acb139dbd2df9e88669fe48e0ed64c14f21b2` |
| `core-mcp-material-final-commit-binding.json` | `38791013b8b2ad5841a7606ec03c79b53a16be86ed8d65cbcb0c2b16a21148d0` |
| `core-mcp-material-cache-compression.json` | `3ff920e1ad2d4a40a85aea4c6b022e0f14e2e1be0632344ffe384324f3c63f6c` |
| `core-mcp-material-after-compression-resources.json` | `86e24d425edefc71b35fc742c7a3bab41aa13f79e3b376748525720b451fe772` |

## 仍待完成

原事件链与受保护 metadata 的读取/registry 发布、owner 显式请求及原 Harness 审批/恢复、正式 Web 接线和安装身份联合证据继续实施。插件、渠道、后续 profile、桌面联合验收和完整 IP01–IP24 的义务保持。本批没有部署、远程推送、服务变更或 UAC，没有刷新 M5/D/Final 发布验收结论。
