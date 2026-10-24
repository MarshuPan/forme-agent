# 发布离线数据与服务 image 组件验收

日期：2026-10-25。实现提交：`639b2b4`，分支 `shu/installed-product`。本记录承接 PRD24 §4.10.1–4.10.2，不是完整升级、安装器发布或 IP14/IP15/IP24 PASS。

## 1. 实现范围

- ReleaseStager 从原管理 journal 与不可变 staging record 重新核对目标，切换前再次验签/核对实际文件；活动版本来自原 journal，不重写初始 installation。
- 固定 `forme-hostctl maintenance-release-data` 私有管道取得原脑独占锁，读取真实配置/SQLite schema，校验 owner/store/事件水位，创建一次性一致性备份并核验 manifest。备份 ref 不作为路径使用；不初始化空脑、不创建数据 key、不调用模型。
- 父进程在同一个绝对期限内监督读写，异常时 kill/wait/join。成功 Release 回执先释放原脑锁；超时或结果不明保留现场，不换号重做。
- Windows service image adapter 完整预检三个内置服务，先以 CAS 持久记录一次性 `ReleaseImagesClaimed`，再更改程序路径；保留账户、依赖、恢复策略及其他配置。部分更改或回执丢失阻止整批重跑。
- 管理 journal 对历史 fact 原始编码验 hash，closed DTO 校验仍独立执行；新增可选字段不改写历史摘要。Committed 先落盘，工作准入后开放。

## 2. 当前源码检查

统一门：`pwsh -NoProfile -File tools/verify-code-quality.ps1`。

| 检查 | 实际结果 |
|---|---|
| Rust workspace/all targets | 169 suite 汇总，965 passed / 0 failed / 5 默认 ignored |
| fmt、workspace check、strict Clippy | PASS |
| 结构/资源/资源字节 Python 回归 | 20/20 PASS |
| 规模与依赖检查 | 665 文件，62 处既有规模债务，未刷新 baseline |
| 前端类型/行为/嵌入资源 | 75/75 PASS |
| bundle 回归 | 9/9 PASS |
| 原生包输入/POSIX 控制流 | 16/16 PASS |
| 实际 Rust 离线签名校验 | 5/5 PASS |
| NTFS 权限组件、只读 firewall 策略 | PASS；后者 25/25，不修改真实规则 |
| npm audit、原创/许可证、diff whitespace | PASS |
| `tools/verify-installed-host.ps1` | 122 个命名场景实际执行并 PASS；零执行被拒 |
| 冻结 workspace/协议门 | 18 crates、依赖图及 99 EventKinds PASS |

新发布专项：staging/生命周期/服务 image 25/25、协议 3/3、真实子进程 transport 3/3、实际加密 SQLite 检查/备份 2/2。源码统一门包含本轮最终 claim 时间绑定；本地忽略日志为 `.forme/installed-release-final-quality-20261025.log` 与 `.forme/installed-release-host-gate-20261025.log`，不提交原始日志。

统一门使用当前工作树，另有 owner/并行的构建资源优化及两个 workflow 未提交改动；它们保留，未混入上述实现提交。当前干净提交的永久 M5/closure 与发布回执另行重验，未用旧报告宣布通过。

## 3. 尚未闭合

独立 OS 托管 updater、Host/Web 停服后的持久接管、新 Host 对原 Checking 的核实与 fresh readiness 仍需实现。生产 Upgrade/Rollback 继续拒绝；新服务 image API 尚未接入完整生产自更新。

本机 shell 非管理员。Windows 组件使用受控 service adapter 与真实临时文件/子进程/SQLite，不是实际 SCM 管理员安装或升级。其他目标的原生 service image adapter 仍 fail closed；所有原生平台及 IP01–IP24 仍需独立证据。未合 main、未 push、未签名发布。
