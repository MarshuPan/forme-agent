# 24.2 Worker 控制面组件验收

实施日期：2026-10-20；复核日期：2026-10-21；分支：`shu/installed-product`。实现提交：`f1122ec`；前置契约提交：`976e958`。这是内部 transport/状态一致性验收，不是 IP01–IP24、安装包或平台强隔离验收。

## 1. 范围

接通 `forme-hostd -> WorkerExecutorHost -> registered development process`，包括版本化受管配置、dispatch/probe/cancel、独立双域 listener 和取消回执重放。保持 18 crates、99 EventKinds 与内部依赖图；没有新增大脑或直接读写 brain store。

默认路径使用不可用隔离器：即使提供完整配置，缺少平台隔离时也不能启动子进程。只有前台显式 `--development-process-workers` 启用普通进程 fixture；服务模式拒绝该开关。普通子进程的完成、退出码和取消只能证明进程控制行为，不能证明文件/网络或身份隔离。

## 2. 证据

| 测试目标 | 核验内容 |
|---|---|
| `forme-hostd` 单元 | HTTP framing/浏览器请求拒绝；SCM 模式禁止开发降级。 |
| `installed_host_transport` | 原维护 HTTP 认证与只读操作幂等继续成立。 |
| `installed_host_worker_transport` | 真实 host/worker 子进程；跨 token 与跨 listener 拒绝；默认隔离缺失零执行；并发八次同 job 只有一个追加 marker；错误 schema/绑定/路由/过期均零 admission。 |
| `installed_worker_config_contract` | closed/有界 JSON、scope/profile、实际路径和受保护文件互斥；Windows 大小写别名拒绝。Unix link 用例仅在 Unix 编译/执行，本机不冒称通过。 |
| `installed_worker_contract` | plan/scope/epoch/nonce/deadline、终态取消、并发取消一次、admission/cancel 顺序、重启 Unknown、取消回执重放。 |
| `installed_worker_process_contract` | 注册入口的真实完成/失败/取消 receipt。 |

上述目标在 Windows 本机共 22 个测试。`tools/verify-installed-host.ps1` 将它们与此前宿主测试合并，51 个命名测试逐项真实执行并全部通过；fmt、workspace check/test、strict clippy、compliance doctor 与冻结图检查已通过。全量测试为 141 个 suite、657 passed、0 failed、5 默认 ignored；ignored 不计通过，也不代表已运行真实模型/安装场景。

最终 Unicode 路径校验补丁已通过宿主专项与 strict clippy。最终源码的 `tools/verify-m5.ps1` 正在重新执行，完整输出保存于本地忽略目录 `target/installed-product-evidence/worker-control-m5-20261021.log`；在取得退出状态前，不声明该长链通过。历史 M5 发布回执保持原样，若与产品增量不同，须区分运行回归与历史发布输入差异。

## 3. 数据与边界

测试使用独立临时目录、自建 fixture/token 和真实 loopback 连接。未加载真实模型配置，未读/迁移 `.forme`，未注册 SCM 服务，未创建账户/修改防火墙/开机项。产品 journal 与脑事件源完全分开；受管配置结构校验不替代 ACL、文件身份重查或抵御同权限恶意进程。

仍未完成：原生 IPC peer identity、逐 job 低权限身份/ACL/Job Object/cgroup/macOS sandbox、网络隔离与会话 helper、完整安装升级恢复、账号与 Web 工作台。IP01–IP24 继续 NOT_RUN；不凭此报告声明 Windows/macOS/Linux 原生产品支持。
