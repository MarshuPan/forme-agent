# 升级服务状态传播与期限组件验收（2026-11-09）

分支 `shu/closure-inventory`，实现提交 `3803069`；此前 `4a89f9d` 已在 PRD24 §4.10.3 写明实施约束。本次复用既有服务观察器，不改变公开 DTO/trait、产品 journal、18-crate 图、99 EventKinds 或五个原生安全代码边界。

## 1. 已复现的问题

升级专用适配器在 SCM 接受一次 start/stop 后，只容忍 pending 或目标状态。SCM 尚未发布 pending、查询仍返回原 Running/Stopped 时，它立即拒绝；普通服务观察器已能在原期限内容忍这种传播。旧循环还接受相反方向的 pending。先将原循环提取到私有 seam，再运行自建状态序列，实际为 **4 passed / 3 failed**；失败原件保留，没有用修复后的结果覆盖。

这是生产观察路径的组件复现，尚未读取本机原升级的受保护诊断，不能把它断言为本机故障的唯一原因。

## 2. 实现边界

- 升级 start/stop 保持原单次控制，后续只观察。它将 SCM 状态映射到既有 typed 状态，共用普通服务的有界 settle 逻辑。
- 每轮先核对原固定 image/account/recovery，再查询状态。只接受本次方向的 pending；pending 后回到原状态、相反 pending、暂停/未知、查询或配置核验失败均停止。
- 观察前后、耗时配置核验之后及最终独立观察之后均检查同一绝对 deadline。旧状态、pending 或晚到终态不能延长授权。
- 未核实结果保留原操作，不补发服务命令、不更换 operation、不修改 journal 或自动恢复权限。

八项新增场景覆盖旧状态传播、立即终态、相反方向与回退、固定配置漂移、查询错误、已过期、原期限不续期及慢查询/慢核验不能补报成功。原共享观察器三项回归也在完整测试中逐名通过；测试没有调用本机 SCM 控制、停止服务或执行升级。

## 3. 验证与原件

最终七阶段在本地时间 **12:40:45–13:01:48** 实际结束，全部 exit 0：fmt、workspace all-targets check/test、strict Clippy、专项、质量与完整合规。完整 workspace 为 **189 targets、1,363 passed / 0 failed / 22 默认 ignored**；八项专项不重复叠加计数。质量门为 901 files / 63 existing size debts，未放宽基线。

独立 verifier 在 13:01:57 核对四份实际源码、七阶段日志、原三项失败、八个新增命名场景、原共享三项、全量计数和资源报告，结果 PASS。实现提交前再次核对源码摘要。固定 target 空间报告 PASS：约 45.2 GiB 逻辑 / 16.9 GiB 分配，低于原 64 GiB 预算；本轮没有删除缓存或修改资源阈值。

工件位于本工作树 `target/closure-inventory-evidence/`：

| 工件 | SHA-256 |
| --- | --- |
| `release-observer-original-reproduction.log` | `5a066d3dd8adbc1f594dec2d4803152721c393b4a36ef7e699a0b1bf2768ce78` |
| `release-observer-checks-process.json` | `e09bde03c71ce9f9eda944e62111e620fc34d42798aecbaec40a74933ea55126` |
| `release-observer-final-test.log` | `3b43aa2db751a0badef034eaaaca9fd1336bbcd2683595281311fcfafa4e25b3` |
| `release-observer-evidence-verification.json` | `189923ef0013a91671704d3eb614c25907fa5fc2910661519a66b43cd3c11ff9` |
| `release-observer-build-resources.json` | `12d4a8f27955bf07944699f74dead42f18cff927e6604133ed937fb9d993eb49` |

## 4. 原生安装与剩余义务

本次未部署修复、未刷新完整 Final/M5，未关闭 IP14–15/IP24。原 `maintenance:correction-scope-upgrade-20261109` 仍只核实原号，不重发；已有清空限定 ForMe 测试实例的 owner 授权不等于把原升级裁决成成功。

当前普通命令进程未提升，Windows UAC 仍启用；本轮未修改全机 UAC、创建提权任务或改动系统信任。新开发候选与有界重装脚本正在准备，未执行原生变更。受管消费者、桌面授权和桌面结果三份冻结提案以及完整原生义务继续见[剩余审计](installed-product-remaining-obligations-audit-20261109.md)。
