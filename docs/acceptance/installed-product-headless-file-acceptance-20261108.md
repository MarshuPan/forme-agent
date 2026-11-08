# 安装版无桌面依赖文件 worker 验收（2026-11-08）

本记录承接 prd/24 §4.1.1d/e、architecture/10 与 canonical §29.10。实现提交为 `ec56b71`；文档契约为 `c7ab71a`，依赖审查为 `002f5bc`。本轮尚未部署新候选，完整 IP10/IP14/IP24 不计通过。

## 1. 原问题与变更

安装版原文件请求 `run:1794135880315062200:2` 的独立 Host 观察为进程退出 `0xC0000142 / DLL_INIT_FAILED`、结果缺失、目标未生成，lease 清理已完成。该原请求已由 owner 显式 Cancelled 收口为 Aborted，重放没有新增 action；历史 Unknown 保留，不再派发原 job。

原 executord 的文件模式仍链接桌面 DLL。仅用一个薄入口调用 execution 库也未消除这些依赖，因此该尝试不作为修复。当前新增 `forme-file-worker` 可执行入口，直接编译仓库同一份文件实现；安全打开文件句柄的硬链接计数通过 Windows-only、精确锁定的 `winapi-util 0.1.11` 公共 API 查询。没有新增内部 crate/依赖边、DTO/trait、EventKind 或 unsafe 边界。

新完整包含七个程序。新安装选择独立文件入口；历史六程序配置继续可读，只有独立签名并验证的 active manifest 列出新入口时才选择它，不改写旧配置、不回退到未列出的文件。恢复基线保留历史摘要算法，并为新布局绑定第七个程序，后续移除或替换会被拒绝。Windows 原生打包前另检查实际 PE64、控制台子系统、导入表及有界 RVA/区段，拒绝延迟导入和桌面 DLL；自建结构 fixture 不是原生执行证据。

## 2. 源码证据

固定证据目录为 `target/native-legacy-review-20261106/`。

| 检查 | 实际结果 | 日志 |
|---|---|---|
| 完整 Rust workspace | 183 个测试目标，1304 passed / 0 failed / 16 默认 ignored | `headless-m5-bounded-gate-20261108.log` 的 complete workspace tests 段 |
| fmt、workspace check、strict Clippy、S1–S99 场景与 golden | 已执行通过 | 同上，workspace tests 之前 |
| Python 回归 | 95 tests，OK，1 skipped | 同上，complete Python gates 段 |
| 原创性、许可证、复制检查 | PASS | `headless-final-compliance-20261108.log`；M5 日志也有独立 PASS |
| 代码规模棘轮 | PASS，880 files / 63 existing size debts | 当前 `tools/code-quality.py` 实际输出 |
| 前端 | 115 passed，依赖 audit 0 vulnerabilities | `headless-frontend-verify-20261108.log`、`headless-frontend-audit-20261108.log` |
| 实际 Rust 签名验证与 POSIX 包控制流组件 | 14 passed / 0 failed / 0 skipped | `headless-signature-and-posix-20261108.log` |
| 受管进程组件 | 4 passed / 0 failed；普通交互进程身份 | `headless-managed-product-process-20261108.log` |
| NTFS、原生 Inno 退出夹具 | 10/10、2/2 PASS；不改变已安装服务 | `headless-ntfs-20261108.log`、`headless-inno-20261108.log` |
| 真实 Low AppContainer 取消/失败/替换反例 | 1 passed，三个分支均保留原文件、无 Host 回写 | `headless-native-negative-file-entry-20261108.log` |

首次 M5 调用在两个 harness 测试目标链接时遇到 Windows `os error 1450`；之后以 `CARGO_BUILD_JOBS=2` 成功链接并运行完整 workspace，原失败日志 `headless-m5-gate-20261108.log` 保留。第二次 M5 调用的最终 S99 审计因 `winapi-util` 缺少依赖表格行而失败，其余审计项通过；审查正文、NOTICE、borrowing record 已存在。本记录补齐同一审查的表格行，后续只重验发布审计，不把第二次完整调用改写为 exit 0。

## 3. 当前原生验收边界

修正版签名候选、实际受管升级和新文件矩阵尚未执行。待执行脚本为 `prepare-headless-candidate-20261108.ps1`、`complete-headless-native-window-20261108.ps1` 与 `installed-headless-file-matrix-20261108.ps1`，均在上述证据目录；三者语法检查通过。

候选准备须绑定干净完整 commit，独立核对七个 release/bundle/signed image 的相同摘要及实际 release PE 导入。一个管理员批次只入库该签名候选、正式 stage/upgrade 各一次，核对 owner/store/config/旧 run 历史后，依次执行新的 DeniedWrite、GrantedWrite、Read、Delete；包括审批前零动作、参数摘要替换拒绝、批准后严格一次、独立文件观察和重放零新增。任一步失败即保存原操作，不另派相同副作用。

当前桌面 DLL 关联是有证据支持的修复假设；必须取得新安装版 LocalSystem/AppContainer 的真实结果才能确认。普通进程、结构 PE fixture、源码回归和开发签名均不替代这个验证，也不代表其他平台或正式发布已通过。
