# 原生打包入口与 POSIX 权限组件验收

2026-10-23，`shu/installed-product`，实现 `b16997c`，承接 `prd/24 §4.4.1`。本组件提供可运行的原生编译入口、固定首次安装权限和失败阻断；没有生成已签名可发布安装包，也没有操作本机生产服务或 owner 数据。

## 实现

- `build-native-package.mjs` 对六个声明目标检查本机 OS/CPU、五个执行文件头、闭合文件集、逐文件摘要、派生索引与显式编译器 SHA pin，独立快照后执行固定编译器参数。拒绝交叉构建冒充原生构建、链接/硬链接、旧输出、输入交叠、额外文件和改变的工具。失败保留现场，不写成功回执；成功回执仍为 unsigned/uninstalled、release_ready=false。
- Windows Inno 编译只读取快照。deb/rpm/pkg 布局携带实际服务文件、前后安装钩子、Web 和许可证；macOS/发行版/CPU 在解包前拒绝不符目标。原生包覆盖既有数据被阻断，升级必须进入独立生命周期事务。
- POSIX helper 只接受固定平台/阶段和默认产品目录；Host、Brain、Web 与 bridge 权限分域。原子配置替换和新凭据 revision 继承安装器选择的只读服务组。Linux 移除按 Brain→Web→Host 停止、观察 inactive、再 disable；不吞掉停止/观察失败。
- 修复真实 bundle→signer 接缝：仅精确派生 SHA256SUMS 可作为额外索引，其他额外文件仍拒绝。当前 Rust release verifier 实际验证新 signer 输出和内容置换拒绝。

## 验证

| 层次 | 本批结果 |
|---|---|
| Rust | `cargo test --locked --workspace --all-targets`，155 suites，802 passed / 0 failed / 5 默认 ignored；日志 `target/posix-access-workspace-current.log`。Unix-only 新权限测试在 Windows 不执行。 |
| 静态门 | fmt、workspace check、strict Clippy、默认完整 compliance、diff whitespace 通过；结构门 508 文件、62 个既有规模债务，未刷新基线。 |
| 打包/签名 | 设置 `FORME_RELEASE_VERIFIER` 为本轮构建的 hostd 后，native_package、posix_packaging、product_bundle、signed_release 合计 28/28，通过实际 Rust 验签，无 skip。布局/架构头 fixture 不执行、不代表 native compiler 或目标平台。 |
| 长门 | `target/installed-product-fixed-m5-current.log` 在固定源码 `8b7f0f4` 跑到 S99；此前运行/合规门通过，但 S99 因 `tokio-rustls` 缺正式依赖表行失败。此处补清单，不改写失败为通过；后续固定源码重新审计。前次语法错误日志也不作为通过证据。 |
| Windows 原生编译 | 真实 Inno ISCC 编译完成，开发候选 `forme-0.0.1-windows-x86_64.exe`，88,947,246 bytes，SHA256 `00448599ac5e599738893525c9b0c52a4d91d8ca329db0e383d24830f04f8e14`。bundle 摘要 `de0deb8525a0c01787e3a4e6affc711e25b7090429588294cb2ad77cfe30aecc`，回执明确 development=true、unsigned/uninstalled、release_ready=false。只对派生二进制去调试信息，原 debug 文件未改；安装器未运行。 |

编译工具来自隔离解压的非官方 NuGet 包 `Tools.InnoSetup 6.6.1`，包 SHA256 `9c994256f6fb3ed7f28e9637b97a3a328c2a3d1d5ba57537ac6b7dcc414ddbe2`；实际 ISCC SHA256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc`，Authenticode Valid，签名者 Pyrsys B.V.。已检查工具许可；仅主编译器显式 pin，未全量 pin 相邻工具文件，版本标签为操作员元数据，不冒充自动版本探测。工具在临时开发目录使用，未安装到系统、未作为产品组件再分发。

## 尚未完成

未运行 dpkg-deb/rpmbuild/pkgbuild/productbuild；未配置正式发布工具链、签名证书或公证凭据；未原生安装、注册真实账户/服务、验证平台有效拒绝、执行 IP01–IP24。Docker daemon 未运行，现有 WSL Ubuntu 26.04 也不是声明的 Ubuntu 24.04 目标。本组件不扩大支持平台清单。

完整恢复切换、升级回滚、强隔离 worker/桌面 helper、其余设置和联合验收继续属于同一总目标，不因本组件提交而关闭。
