# Windows 安装权限模块加载修正（2026-11-07）

承接 PRD24 §4.4/§7 和 IP01/IP24。本次只修正随包 ACL helper 的原生模块解析，不改冻结 DTO、trait、EventKind、依赖边、权限矩阵或恢复协议。

## 1. 根因证据

此前真实签名候选 `forme-windows-x86_64-20261106-07d0bdc` 的一次安装在 `PrepareToInstall` 返回 exit 7；有界诊断是 `stage=5 source=2 line=101 hresult=-2146233087 category=13`，位置为 `service-access.psm1` 的 `Get-Acl`，不是 ACL 不匹配断言。

本轮先确认工作树干净、HEAD `c29fee1`、当前进程未提权、SCM 零 ForMe 服务且没有遗留 consent 或安装进程。上轮被取消的比较脚本没有运行，故其输出文件仍不存在；随后才重新发起同一个只读比较。

23:25（Asia/Shanghai）实际管理员父进程 PID 59680 运行 PowerShell 7 预检并启动原 Prepare-only 夹具。该父进程与真实安装窗口一样加载了现代 Security 模块、核对原 signer，然后启动 Inno→Windows PowerShell 子进程。固定两个根均存在，参数精确匹配；原 helper 仍在阶段 5、第 101 行报 `CommandNotFoundException / CouldNotAutoloadMatchingModule`，分类 13。夹具强制在 payload 前终止，无根或服务写入。

这证明原已知失败可由继承的跨 PowerShell host 模块发现环境复现。此前直接启动夹具成功不能排除父进程环境差异；本次错误是 `Get-Acl` cmdlet 的模块加载失败，不是目录消失、DACL 不符合或管理员令牌缺失。证据位于 `target/installer-entry-probe/exact-preparation-v2/failure.json`、`nested-result.json` 和 `native-nested.log`，原有失败均保留。

23:36 再以实际 PowerShell 7→Windows PowerShell 的非提权只读子进程比较，保留真实继承环境而非模拟：发现一个原生和一个非原生 Security 模块，首匹配不是当前 host 的模块；默认 `Get-Acl` 实际得到相同 `CommandNotFoundException / CouldNotAutoloadMatchingModule`。同一子进程显式导入 `$PSHOME` 模块后，对同一目录的 ACL 读取成功，解析来源精确等于原生 manifest。`target/installer-entry-probe/inherited-security-environment.json` 保留该正反观察；没有服务或产品根写入。这补足了模块来源与修正机制的证据，仍不是完整签名包安装结果。

## 2. 实现与正反检查

`service-access.psm1` 在导出权限函数前，按当前 `$PSHOME` 的绝对路径显式加载随该 PowerShell host 分发的 `Microsoft.PowerShell.Security.psd1`，加载失败继续拒绝。它不搜索继承的模块路径、不安装依赖、不改变 UAC、目录 owner/DACL 或服务身份。

新增反例在独立临时目录放置一个仓库自建、明确不兼容的同名 Security 模块，并只给子进程继承该模块路径：原生 `Get-Acl` control 必须实际得到 `CouldNotAutoloadMatchingModule`；加载修正后的权限模块必须从 Windows PowerShell 自身目录解析 cmdlet，并回读实际 NTFS ACL。测试核对具体 module path，不以“没有抛错”代替加载来源。子进程先持有 handle，再有界等待并采集真实退出码，避免 Windows PowerShell 对短进程返回空 ExitCode 的包装问题。

Windows PowerShell 5.1 与 PowerShell 7 的权限组件均 10/10 PASS，包括原有继承、链接/硬链接、错误路径、诊断不覆盖、原 hostctl 布局及新增模块环境反例。规模棘轮保持；这不是原生安装或 IP01/IP24 通过。

23:42 完整源码门实际 exit 0：182 个 Rust 测试目标，1301 passed、0 failed、16 默认 ignored；strict Clippy、前端 115/115、bundle 10/10、package 18/18、离线签名 5/5、只读 firewall 25/25、原生安装器退出夹具 2/2 和完整原创性/许可检查均通过。生产 Rust、18 crate 图、99 EventKinds 与五处已审计 unsafe 边界不变。`native-security-source-gate.json` 核对实际日志摘要，不把源码门升级为原生安装结果。

本轮证据摘要：

| 证据（本工作树 `target/` 下） | SHA-256 |
| --- | --- |
| `installer-entry-probe/exact-preparation-v2/failure.json` | `adeb932813d607ac746abb997547f5f589a4c2e3aaefbf92d0ee042ad4b2fbfb` |
| `installer-entry-probe/exact-preparation-v2/nested-result.json` | `d36c9a7a5e771f37c26ce3f7cb60b9a1869ee095a9270c6af55f9c7304ac06d7` |
| `installer-entry-probe/exact-preparation-v2/native-nested.log` | `30d5b8bd7eed58b4445c41df90632d34682a8cbf10575540c2fcb0c073cc973a` |
| `installer-entry-probe/inherited-security-environment.json` | `a6e55bd7413554e17f1d577e5c2bc1763c24d618a409bb28096dff3d4f39f064` |
| `native-legacy-review-20261106/native-security-source-gate.log` | `aa86c428e4b5bb2fc33aea6b11c9cfdc09b2c5c2b42a23fdc2c9a1e7b796f5f5` |

## 3. 原生安装与当前边界

固定的修正后 Prepare-only 比较已经准备并通过脚本解析检查，仍使用同一父进程环境和两个现有受保护空根；没有实际安装或权限放宽。23:30 发起的 UAC 随后被 Windows 返回“操作已被用户取消”，launcher session 15027 没有取得有效管理员 PID，也没有 `native-security-fixed/nested-result.json`，因此不能宣称修正后原生比较已执行。

后续从干净修正提交生成新开发签名候选与独立升级候选，只运行一次固定安装窗口。成功后沿原正式 API 完成认领/配置自动启动、拒绝与批准文件读写删、停服/离线备份/启动及签名升级；失败或 Unknown 保留原操作和回执，不重复派发。现有空根安装不是 clean IP01；完整浏览器、冷启动/注销、回滚/CommitRestore、LAN 与其他平台证据继续独立记录。

所有已完成门、候选和原生结果在本报告后续追加，不能使用“准备完成”替代实际安装。当前本机没有可用 ForMe 服务，整体目标未完成。
