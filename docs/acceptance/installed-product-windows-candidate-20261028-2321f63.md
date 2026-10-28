# Windows 当前主线安装候选（2321f63）

2026-10-28，基于本地 `main@2321f63cbfec4642b2c8e6e3a7d22f434f50bffd` 生成。本候选承载当前统一 Web 控制面、Host 自动协调 Brain 的源码输入；本记录只证明 release 构建、bundle 闭合集和本机原生安装器编译，尚未签名、安装、注册服务或进入 IP01–IP24 验收。

## 构建证据

| 项目 | 结果 |
|---|---|
| 目标 | Windows x86_64；本机原生 release 编译 |
| Rust 构建 | `cargo build --release --locked --workspace`，退出码 0 |
| Bundle | 18 个文件，含六个程序、Web 资源与许可文件 |
| Bundle content digest | `sha256:a9ef82e56039914fa3f94fd880afba07e8dc2d8e9f1918e814095fbd7234b3a9` |
| Native compiler | Inno Setup 6.6.1；SHA-256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc` |
| 安装包 | `target/product-bundles/forme-windows-x86_64-20261028-2321f63-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| 安装包大小 | 73,293,592 bytes |
| 安装包 SHA-256 | `832ad6680bdaa4a009153f7483c5fb3dcf522fe46705bfc38adb6fc4c7d5a9f8` |
| 签名/安装 | `not_performed` / `not_performed` |
| release ready | `false` |

bundle inventory、`SHA256SUMS` 和 `native-build-receipt.json` 均绑定上述 release id 与 bundle digest；安装器构建步骤对输入 bundle 和打包后 bundle 都完成了闭合集复核。构建过程未读取、打包或上传 owner 数据、`.forme`、模型配置或密钥。

## 产品边界与未验证项

- 对外仍是一个 ForMe 软件设备：一个 Product URL、一组系统服务、一个 Owner、一个 ProductionRuntime/主脑和一套持久数据；Web 是统一控制面，不是外部 Agent 的桥接页。
- 当前候选没有覆盖 `C:\Program Files\ForMe` 或 `C:\ProgramData\ForMe`，没有改变现有 Host/Web/Brain/updater 服务状态。
- 当前安装现场仍是旧候选；本候选尚未证明 SCM 下 Host 自动恢复 Brain、配置保存后的真实自动启动、真实模型、worker 隔离、升级/回滚或 IP01–IP24。
- Linux/macOS 原生服务和安装包仍保持既定有限边界，不能由 Windows bundle 构建证据替代。

## 可复现命令

```powershell
py -3 tools/build_resources.py check
$env:CARGO_BUILD_JOBS = '2'
cargo build --release --locked --workspace
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id forme-windows-x86_64-20261028-2321f63 --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-2321f63 --development
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-2321f63 --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-2321f63-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

对当前已安装实例执行升级、替换程序或启动服务，需要针对本候选另行确认；此前对其他候选的授权不自动覆盖本候选。
