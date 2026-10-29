# 当前 Windows 原生 worker 验收记录

- 日期：2026-10-29
- 当前源码：`main@429801bc5218f2c6f2446ba4222f572c3d726a5a`
- 范围：临时目录中的 Windows AppContainer、私有 workspace overlay、`forme-executord --installed-file-worker` 与 Host 结果通道。
- 不包含：安装/注册 SCM 服务、修改现有 ForMe 安装、读取 owner 数据、真实模型调用或 IP01–IP24 联合验收。

## 1. 真实 AppContainer 合同

命令：

```powershell
cargo test --locked -p forme-execution --test installed_windows_appcontainer_contract -- --ignored --nocapture
```

结果：`4 passed, 0 failed`。

覆盖的真实场景：

- Low AppContainer 使用私有 workspace 副本，Host 核验后才回写；
- worker 失败、取消和 owner 冲突不写回原 workspace；
- Host 进程被终止后恢复原 lease，不重放 worker；
- 结果先封存，再撤销 ACL/profile 和清理临时副本；未封存结果保留证据。

所有场景使用测试临时目录和仓库自建 worker fixture；没有触碰 `C:\Program Files\ForMe`、`C:\ProgramData\ForMe` 或既有服务。

## 2. 真实文件 worker 入口

命令：

```powershell
cargo test --locked -p forme-execution --test installed_file_worker_contract -- --ignored --nocapture --skip native_file_entry_rejects_a_symbolic_link_target
```

结果：`2 passed, 0 failed`。

覆盖：

- `forme-executord --installed-file-worker` 的真实读/写/删操作；
- 固定 `WorkerFileTarget` 映射与 Host-only writeback；
- 取消、失败及执行后替换不改变原文件；
- Host 结果 manifest、分块读取、预算绑定和加密结果封存。

## 3. 未验证边界

全量 ignored 运行还尝试了 `native_file_entry_rejects_a_symbolic_link_target`，但当前非提权测试进程创建 Windows symbolic link 返回 Win32 错误 `1314`（客户端没有所需特权）。该场景仍为 `NOT_RUN/ENVIRONMENT_BLOCKED`，不是代码通过，也不是失败；测试自身已声明需要额外 symbolic-link 权限。本记录不把它折算为原生隔离完整通过。

本记录仍不能提升以下状态：真实安装后的 SCM/LocalSystem 服务账户、系统安装与升级/回滚、桌面 helper、Linux/macOS 原生服务、真实 provider、跨设备 LAN TLS，以及 IP01–IP24。它只更新当前源码的 native worker 证据，不能替代整机产品验收。
