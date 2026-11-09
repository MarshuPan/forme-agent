# 隔离 PTY 与真实 backend I/O 组合验收（2026-11-09）

实现提交：`e9da7084de9e4607c45959061367f11e2941956d`，分支 `shu/closure-inventory`。本轮落实[机制设计稿 §4.1](../architecture/proposals/installed-isolated-pty-mechanism-spike.md#41-普通-backend-与隔离机制的组合验证)，让既有 `PtyBackend` 实际运行在测试专用 Low AppContainer/Job 中。它补齐此前普通 I/O 与隔离机制分开取证的接缝；不启用生产 PTY、不改变冻结协议、18-crate 图、99 EventKinds 或五个 unsafe adapter，也不是 IP10/IP11/IP24 的通过声明。

## 1. 真实组合及独立观察

自建 runner 使用现有公开 `PtyBackend`，没有另写一份 I/O 算法。runner 挂起进入原 Job，完成 Low AppContainer、capabilities 和特权回读后才 resume。子程序报告真实 terminal/PID 并等待 gate；外层持有原句柄，独立核对 runner、conhost、child 的 image、creation time、token 和同一 Job 后，才允许测试正文执行。

每个 experiment 使用独立私有副本、两份自建不可读 sentinel 和本次 loopback listener；实际连接拒绝、listener 零接纳、原 sentinel 字节不变、父 PATH/代理/自建凭据标记不继承分别核对。测试仅转交本次探针输入文件的路径，不接入真实秘密、用户正文、已安装 workspace 或生产引用输入通道。

八个 I/O 场景均要求唯一 Started、准确终态、CapabilityEvidence、无 Unknown、原进程树退出、Job 清空及原 ACL/profile 清理。失败场景还核对实际原因，不能用任意 ActionFailed 替代超时、输出预算、append 拒绝或 exit 7。输出拒绝在测试 gate 后才生效；输入取消先观察正文启动标记。

backend 的配置期限为 3500 ms，包含启动与外层观察 gate。I/O 结果对返回时间另断言不超过 6500 ms，超时场景不少于 3500 ms；原 worker 等待继续使用 10 秒轮询期限，独立进程枚举有 4 秒期限。这些是测试约束和本次采样，不是所有 OS 调用的性能保证或生产清理故障已闭合的声明。

## 2. 最终实际执行结果

Windows build `26200.9711`，普通权限测试进程执行。完整 all-targets 构建后，使用该轮实际执行过的 execution 单元测试程序显式运行：

```text
forme_execution-45880ad7cee56e98.exe --ignored installed::windows_appcontainer_native::pty_probe:: --test-threads=1 --nocapture
```

本地时间 **09:13:56–09:14:51**，实际 **4 tests passed / 0 failed**，产生 **11 份不同 experiment 回执**：八个 I/O case，加原文件派生限制、正常终端往返和后代取消三个原场景。不是 11 个 Rust test，也不把这次显式结果叠加进默认 workspace 数字。

| I/O 场景 | 实测返回 | 真实结果与断言 |
| --- | --- | --- |
| 输入不读、超时 | 3854 ms | ActionFailed，原因 timeout；正文确已进入。 |
| 输入不读、启动后取消 | 1731 ms | ActionCancelled；先观察原正文，再发取消。 |
| 持续输出、慢观察者 | 4541 ms | ActionFailed，原因 timeout；持续输出未饿死期限检查。 |
| 严格输出预算 | 1487 ms | ActionFailed，原因 output-budget；不是其他启动错误。 |
| 正文输出 append 拒绝 | 1498 ms | ActionFailed，原因 append-rejected；正文已进入。 |
| 正常 burst 与尾部 | 1498 ms | ActionCompleted；149,037 字节终端输出，尾部标记存在，无截断。 |
| 截断预算 | 1515 ms | ActionCompleted；恰为 4096 字节，唯一截断标记，原清理完成。 |
| 错误 exit | 1472 ms | ActionFailed，原因 exit-7；无完成或 Unknown。 |

上述八项各独立观察三个实际进程，最终均退出；后代取消原场景另外观察 grandchild。原 File profile 反例仍被 OS 以 `ERROR_CHILD_PROCESS_BLOCKED`（367）拒绝，实验派生许可没有暴露给产品配置或服务。

## 3. 完整源码回归与绑定

本地时间 **08:57:48–09:13:42**，fmt、workspace all-targets check/test、workspace strict Clippy、code quality 和完整 compliance 均 exit 0。Rust 为 **187 targets、1,329 passed / 0 failed / 22 默认 ignored**。新增矩阵默认 ignored，只在上一节的显式命令中实际执行；质量门为 895 files / 63 existing size debts。

完整检查绑定 `f0ecc4f` 加四个精确源码快照，实现提交只包含这四个文件。最终场景前后核对源码、fixture、测试程序和六份检查日志；独立 evidence verifier 又核对默认测试总数、11 份回执、实际 case 唯一性及 Job 清理，结果 PASS。

先行八个实验在 08:51:31–08:52:49 通过，使用 fixture `8b3675bd…`；之后补强了失败原因/时间断言，完整构建也改变了 fixture。先行原件保持，不用其结果替代最终源码。最终 fixture SHA-256 为 `ca30228e27e2fd673b941f9c3e9a4889537a792ee0cca466606c0a9f8ea37aa9`；测试程序 SHA-256 为 `24b7bc58bd32bdada95fd2e91783045837fea2c511cfd09275100d5715084064`。

固定证据位于本工作树 `target/closure-inventory-evidence/`；最终 process 回执列出全部 11 份原件的路径和摘要：

| 工件 | SHA-256 |
| --- | --- |
| `isolated-pty-io-checks-process.json` | `d5f256d642c706b30b9933b3eef614458e05ec07686d918a56ee3873e8f3c119` |
| `isolated-pty-io-final-process.json` | `632ac8cee21a95b1a15e67367176d1aab5783f5defa3e443da05922dd265e20e` |
| `isolated-pty-io-evidence-verification.json` | `b5bb5934003d7098b4c1750c2dc0e46defcd19bc44c612fe178309f410b051b8` |
| `isolated-pty-io-check-test.log` | `0a6c1af78eca5dc4d16d0b2b915d0d53ec5360d1bfc3c4e91cbae48e99639583` |
| `isolated-pty-io-final-native.log` | `3bef2a590ecbb40cb7dc5cb942a3f55eedbe53b38cb19f6d674c15dd19b57ef4` |
| `isolated-pty-io-focused-process.json`（先行原件） | `f314a755724592179fc39bb5e062ae5c616884e6ed21319d7a614e5ecc2805b8` |
| `isolated-pty-io-post-native-build-resources.json` | `ba0a469a9467a96812fb264f2727426a28fd18e56a055446da07218f40e50ea3` |

所有 Cargo 阶段前的 20 GiB 空间门通过；末次资源报告 PASS，缓存约 43.3 GiB 逻辑 / 18.8 GiB 分配，未超过 64 GiB 预算。本轮未删除缓存或放宽阈值。

## 4. 仍开放的安装义务

这组证据关闭的是测试专用隔离环境中的八项 I/O 组合场景。PTY 初始化失败、线程资源耗尽、runner/OS 关闭异常的完整故障注入仍开放；不能从正常清理通过推断这些切点已验证。

生产专用 placement、Host-issued grant、真实 OS 用户会话、不可变引用/秘密输入、受保护结果、overlay 回写、同号恢复和真实 SCM 联合证据仍需闭合。三份冻结协议提案及 IP01–IP24 状态见[剩余义务审计](installed-product-remaining-obligations-audit-20261109.md)。本次没有安装、升级、启停服务、修改 UAC、刷新完整 Final/M5 或标记整体目标完成。
