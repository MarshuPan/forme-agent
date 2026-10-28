# Windows 当前主线安装候选（d93d9ae）

2026-10-28，基于本地 `main@d93d9ae391e4588adc0898a3c2714224a06c259c` 生成。本候选承载当前统一 Web 控制面、Host 自动协调 Brain 和软件一体机产品边界的源码输入。本记录只证明 release 构建、bundle 闭合集和本机原生安装器编译；它尚未签名、安装、注册服务或进入 IP01–IP24 验收。

## 构建证据

| 项目 | 结果 |
|---|---|
| 目标 | Windows x86_64；本机原生 release 编译 |
| Rust 构建 | `cargo build --workspace --release --frozen`，退出码 0 |
| Web 验证 | `npm --prefix frontend run verify`，类型检查、96 项前端测试和 Web 资源检查通过 |
| Bundle | 18 个文件，含六个程序、Web 资源与许可文件 |
| Bundle content digest | `sha256:242237050b4ebc17bdb06fb4f0c0a283bcbbe5493f3ababd7eff405d7e0ab11b` |
| Native compiler | Inno Setup 6.6.1；SHA-256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc` |
| 安装包 | `target/product-bundles/forme-windows-x86_64-20261028-d93d9ae-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| 安装包大小 | 73,292,643 bytes |
| 安装包 SHA-256 | `e66272014102bd248f76aa3994e5fab7bca8e79616c2961a02c192b1a49d74ff` |
| 签名/安装 | `not_performed` / `not_performed` |
| release ready | `false` |

`bundle-inventory.json`、`SHA256SUMS` 和 `native-build-receipt.json` 均绑定上述 release id、bundle digest 和源构建输入。构建过程未读取、打包或上传 owner 数据、`.forme`、模型配置或密钥。候选目录为新目录，未覆盖任何既有候选。

## 产品边界

- 对外是一个 ForMe 软件设备：一个 Product URL、一组系统服务、一个 Owner、一个 ProductionRuntime/主脑和一套持久数据；Web 是统一控制面，不是外部 Agent 的桥接页。
- Host、Web、Brain 和 worker 仍是同一实例内部的权限、故障和生命周期隔离单元；用户不需要认识内部地址、服务名或凭据。
- 安装后无有效 owner/模型配置时，Brain 可以保持停止；认领并保存有效配置后，Host 必须自动协调 Brain，正常流程不要求用户点击“启动主脑”。

## 当前安装现场边界

当前 `C:\Program Files\ForMe` / `C:\ProgramData\ForMe` 中的安装实例仍是旧候选 `forme-windows-x86_64-20261027-7cb5b8b`。本候选没有覆盖安装目录、替换服务程序、写入配置或改变服务状态；因此本报告不能证明当前安装已运行本候选，也不能证明配置保存后的真实 `BrainReady`。

当前现场的 Web/Host 可达和账号登录证据，与本候选的构建证据分开保存。只有在 owner 针对本候选批准一次受控升级后，才能验收以下闭合链：

```text
有效配置保存
  -> Host 读取同一 Product 实例的配置与凭据
  -> Host 幂等协调 Brain 服务
  -> Brain 以当前候选装配 ProductionRuntime
  -> core-config-ack 与配置 revision 匹配
  -> Web 从 starting 自动进入 ready
```

## 可复现命令

```powershell
py -3 tools/build_resources.py check
cargo build --workspace --release --frozen
npm --prefix frontend run verify
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id forme-windows-x86_64-20261028-d93d9ae --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-d93d9ae --development
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-d93d9ae --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-d93d9ae-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

对当前已安装实例执行升级、替换程序、启动服务或读取受保护配置，需要针对本候选另行执行明确的 owner 操作授权；此前对其他候选的授权不自动覆盖本候选。
