# 独立产品 24.2 真实进程 worker 验收

## 1. 范围

本报告只验收 24.2 中“受安装配置约束的真实子进程执行”这一增量。它建立在 `installed-product-24-2-worker-foundation.md` 的 admission/journal 基础上，不宣称完成原生安装包、系统服务注册、平台强隔离、会话 helper 或 Web 产品。

实现提交：`0021902 feat(execution): run registered workers with durable receipts`

## 2. 已实现

- `WorkerLaunchSpec` 要求安装配置预注册绝对可执行文件、固定参数和 profile；worker job 不能携带或替换程序路径、参数或环境。
- `WorkerLaunchRegistry` 拒绝重复 profile、相对路径、链接/reparse ancestor、非普通文件以及常见 shell 入口。
- `ProcessWorkerIsolation` 为每个 job 建立受保护运行目录，写入版本化 job/permit，清空继承环境，设置精确 workspace 当前目录，并只注入固定的 worker 协议环境变量。
- stdout/stderr 不作为结果通道，直接丢弃，避免任意 worker 输出无限增长产品日志；真正的有界 artifact/result 通道仍由执行后续模块提供。
- `WorkerExecutorHost` 在真实副作用前持久化 `Running`，通过 `probe` 读取子进程退出状态并生成绑定原 job/plan/epoch/nonce 的 receipt；未知结果保持 Unknown，不盲重试。
- 取消使用同一 job 的进程句柄，完成后再次取消不会再次调用子进程；宿主释放时会终止仍由该实例持有的 child。

## 3. 证据

`cargo test -p forme-execution --test installed_worker_process_contract -- --nocapture`：3/3 通过。

- 成功子进程：实际写入独立 workspace，probe 生成 `Completed` receipt。
- 非零退出：probe 生成 `Failed` receipt，不伪装成功。
- 取消：终止长运行子进程，生成 `Cancelled` receipt；重复取消被拒绝且无第二次副作用。

提交前全量门：

- `cargo fmt --all -- --check`：PASS
- `cargo check --workspace --all-targets`：PASS
- `cargo test --workspace --all-targets`：PASS（既有 ignored 场景保持原状态）
- `cargo clippy --workspace --all-targets -- -D warnings`：PASS
- `bash tools/compliance-doctor.sh`：PASS

## 4. 明确未完成

该实现是受注册的进程边界，不是完整平台 sandbox。以下仍不能从本报告推出 PASS：

- Windows 独立服务账户、ACL、Job Object、网络隔离与 SCM 冷启动；
- Linux systemd service/cgroup、账户/ACL、网络隔离与冷启动；
- macOS LaunchDaemon、账户/ACL、sandbox profile、网络隔离与冷启动；
- 子进程树的跨平台强制回收、会话 helper、凭据 slot 和 artifact/result 传输；
- 正式安装器、签名、公证、升级、回滚、恢复、卸载以及 IP01–IP24。

因此本报告结论是：**受注册入口的真实 worker 进程执行 PASS；24.2 平台隔离与独立产品整体仍 NOT COMPLETE。**
