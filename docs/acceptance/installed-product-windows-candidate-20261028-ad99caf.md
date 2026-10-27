# Windows 当前源码安装候选（ad99caf）

2026-10-28，基于 `shu/installed-product` 当前提交 `ad99caf86d6ddd26ce8f5d74fddfb8563101b8cc` 生成。本候选包含当前源码的统一 Web 控制面、Host 自动协调 Brain 逻辑及完整产品服务；本记录只证明 release 构建、bundle 完整性和本机原生安装包编译，尚未签名、安装、注册服务或进入 IP01–IP24 验收。

## 构建证据

| 项目 | 结果 |
|---|---|
| 目标 | Windows x86_64；本机原生 release 编译 |
| Rust 构建 | 已由当前质量门完成；`cargo build --release --locked --workspace` |
| Bundle | `target/product-bundles/forme-windows-x86_64-20261028-ad99caf/`；18 个文件，含六个程序、Web 资源与许可文件 |
| Bundle content digest | `sha256:7a5e3397c64e0f2344c84d22cc0d96e1b39cb778ccdf614d33d1ad2cbea36a28` |
| Native compiler | Inno Setup 6.6.1；SHA-256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc` |
| 安装包 | `target/product-bundles/forme-windows-x86_64-20261028-ad99caf-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| 安装包大小 | 73,290,682 bytes |
| 安装包 SHA-256 | `42d094e3925ca25c40e39eec8cdf7d497629802fd6988ea541eafb02a431cb01` |
| 签名/安装 | `not_performed` / `not_performed` |
| release ready | `false` |

构建回执位于 `target/product-bundles/forme-windows-x86_64-20261028-ad99caf-package/native-build-receipt.json`。bundle 与安装包由现有工具在创建前后核对闭合集、逐文件摘要、Windows PE 头、`SHA256SUMS` 和编译器摘要；构建期间未读取、打包或上传 owner 数据、`.forme`、模型配置或密钥。

## 产品边界

- 对外这是一个 ForMe 软件设备：一个 Product URL、一组系统服务、一个 Owner、一个 ProductionRuntime/主脑和一套持久数据；Web 是统一控制面，不是外部 Agent 的桥接页。
- `forme-webd`、`forme-hostd`、`forme-gatewayd`、`forme-executord` 等是同一实例的内部运行单元。源码中的内部 `Bridge` 仅表示受认证的进程间传输，不构成用户可见的第二产品或第二主脑。
- 当前候选没有覆盖 `C:\Program Files\ForMe` 或 `C:\ProgramData\ForMe`，没有改变现有 Host/Web/Brain/updater 的服务状态。
- 本候选尚未证明 SCM 下 Host 自动恢复 Brain、配置保存后的真实自动启动、真实模型、worker 隔离、升级/回滚或 IP01–IP24；这些必须使用本候选进行受控原生验收。

## 可复现命令

```powershell
py -3 tools/build_resources.py check
cargo build --release --locked --workspace
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id forme-windows-x86_64-20261028-ad99caf --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-ad99caf --development
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-ad99caf --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-ad99caf-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

对当前已安装实例执行升级、替换程序或启动服务，需要针对该候选另行确认；此前对其他候选的授权不自动覆盖本候选。
