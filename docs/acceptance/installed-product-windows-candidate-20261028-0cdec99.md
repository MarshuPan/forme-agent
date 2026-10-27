# Windows 当前源码安装候选

2026-10-28，在 `shu/installed-product` 当前源码提交 `0cdec991a69de74400b5c124850387ec9e7fc79a` 上重新构建。本候选包含配置保存后自动启动 Brain 的 Host 逻辑；记录只证明当前源码的 release 构建、bundle 完整性和本机原生安装包编译，尚未签名、安装、注册服务或进入 IP01–IP24 验收。

## 构建证据

| 项目 | 结果 |
|---|---|
| 目标 | Windows x86_64；本机原生 release 编译 |
| 资源预检 | `py -3 tools/build_resources.py check` PASS；约 148.9 GiB free |
| Rust 构建 | `cargo build --release --locked --workspace` PASS |
| Bundle | `target/product-bundles/forme-windows-x86_64-20261028-0cdec99/`；18 个文件，含六个程序、Web 资源与许可文件 |
| Bundle content digest | `7a5e3397c64e0f2344c84d22cc0d96e1b39cb778ccdf614d33d1ad2cbea36a28` |
| Native compiler | Inno Setup 6.6.1；SHA-256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc` |
| 安装包 | `target/product-bundles/forme-windows-x86_64-20261028-0cdec99-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| 安装包大小 | 73,290,688 bytes |
| 安装包 SHA-256 | `07f91f948215c0f14ec776f35794376b5cbe840735da5068752302c6aa728c8a` |
| 签名/安装 | `not_performed` / `not_performed` |
| release ready | `false` |

构建回执位于 `target/product-bundles/forme-windows-x86_64-20261028-0cdec99-package/native-build-receipt.json`。bundle 与安装包由现有工具在创建前后核对闭合集、逐文件摘要、Windows PE 头、`SHA256SUMS` 和编译器摘要；构建期间未读取、打包或上传 owner 数据、`.forme`、模型配置或密钥。

## 当前边界

- 当前安装中的服务仍使用既有 release；本候选没有覆盖 `C:\Program Files\ForMe` 或 `C:\ProgramData\ForMe`。
- 该候选尚未证明 Host 启动后会在真实 SCM 下自动恢复 Brain，也尚未证明配置保存后的真实自动启动；这些必须用本候选的受控升级/启动观察单独验收。
- 构建成功不等于安装成功，不提升 Host/Web、认领、真实 provider、worker 隔离、升级、恢复或 IP01–IP24 通过。

## 可复现命令

```powershell
py -3 tools/build_resources.py check
cargo build --release --locked --workspace
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id forme-windows-x86_64-20261028-0cdec99 --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-0cdec99 --development
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-0cdec99 --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-0cdec99-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

对当前已安装实例执行升级、替换程序或启动服务，需要针对该候选另行确认；此前对其他候选的授权不自动覆盖本候选。
