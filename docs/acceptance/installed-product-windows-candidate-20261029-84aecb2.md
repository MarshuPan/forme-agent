# Windows 当前 main 安装候选（84aecb2）

2026-10-29，基于本地 `main@84aecb2` 生成。本候选包含 managed Host 配置配对的 fail-closed 修复，承载统一 Web 控制面、Host 自动协调 Brain 和软件 appliance 产品边界。本记录只证明 release 构建、静态资源核验、bundle 闭合和本机 Inno Setup 编译；签名、安装、服务切换和 IP01–IP24 仍未执行。

## 构建证据

| 项目 | 结果 |
|---|---|
| 源提交 | `84aecb2` |
| 目标 | Windows x86_64；本机 release 编译 |
| 资源预检 | `py -3 tools/build_resources.py check`，PASS；构建前约 110.9 GiB 可用 |
| Release 构建 | `cargo build --locked --release --workspace --all-targets`，退出码 `0` |
| Bundle | 18 个文件，含六个程序、Web 资源与许可文件 |
| Release id | `20261029-84aecb2` |
| Bundle content digest | `sha256:06cc7df41fc0a90dfe45d38133000a7edd662ba64de752199303a9d65986ef3a` |
| Native compiler | Inno Setup 6.6.1；工具摘要见 `native-build-receipt.json` |
| 安装包 | `target/product-bundles/forme-windows-x86_64-20261029-84aecb2-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| 安装包大小 | `73,633,710` bytes |
| 安装包 SHA-256 | `74d3da70880b761722f2d7167f2252fdd5c725794750012bcfed6bb9f1955a7c` |
| Inno 编译 | 返回码 `0` |
| 签名/安装 | `not_performed` / `not_performed` |
| release ready | `false` |

构建过程未读取、打包或上传 owner 数据、`.forme`、模型配置或密钥；候选输出使用新的 target 目录，没有覆盖既有候选或安装现场。

## 产品边界与版本原因

- 对外是一个 ForMe 软件 appliance：一个 Product URL、一组系统服务、一个 Owner、一个 ProductionRuntime/主脑和一套持久数据；Web 是统一控制面，不是外部 Agent 的桥接页。
- Host、Web、Brain 和 worker 是同一实例内部的权限、故障和生命周期隔离单元；用户不需要认识内部地址、服务名或凭据。
- 无有效 owner/模型配置时 Brain 可以保持停止；认领并保存有效配置后，Host 必须自动协调 Brain，正常流程不要求用户点击“启动主脑”。
- `84aecb2` 修复了 managed Host 中产品配置模板与 Web 配置凭据必须成对存在的 fail-closed 边界。旧的 `fc4e23e` 与 `6d7eedb` 候选不包含该修复，不能作为当前正式升级目标。

## 当前安装现场边界

当前 `C:\Program Files\ForMe` / `C:\ProgramData\ForMe` 中的安装实例不是本候选；本候选没有覆盖安装目录、替换服务程序、写入配置或改变服务状态。当前现场只读观察到 Host/Web 常驻、Product URL 可达，Brain 仍停止；这不能证明本候选已运行，也不能证明当前安装的 BrainReady、真实 provider、worker 或任何 IP01–IP24 已通过。

本候选需要 owner 对**新包**另行确认受控升级；此前对 `fc4e23e` 的批准不自动覆盖本候选。升级完成后才能验收：

```text
候选切换
  -> WebAvailable
  -> 认领/配置保持
  -> Host 自动协调 Brain
  -> BrainReady
  -> Web 对话、审批、工具和恢复链路
```

## 可复现命令

```powershell
py -3 tools/build_resources.py check
cargo build --locked --release --workspace --all-targets
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id 20261029-84aecb2 --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261029-84aecb2
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261029-84aecb2 --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261029-84aecb2-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

