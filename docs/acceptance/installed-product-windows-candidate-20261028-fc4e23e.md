# Windows 当前 main 安装候选（fc4e23e）

2026-10-28，基于本地 `main@fc4e23e6d173be3893d7a4afe390ec84f96ef33c` 生成。本候选承载统一 Web 控制面、Host 自动协调 Brain 和软件 appliance 产品边界的最新源码输入。本记录证明 release 构建、bundle 闭合集和本机 Inno Setup 编译；它尚未签名、安装、注册服务或进入 IP01–IP24 验收。

## 构建证据

| 项目 | 结果 |
|---|---|
| 目标 | Windows x86_64；本机 release 编译 |
| 资源预检 | `py -3 tools/build_resources.py check`，PASS；剩余空间约 122.5 GiB |
| Release 构建 | `cargo build --locked --release --workspace`，退出码 `0` |
| Bundle | 18 个文件，含六个程序、Web 资源与许可文件 |
| Release id | `forme-windows-x86_64-20261028-fc4e23e` |
| Bundle content digest | `sha256:7dcc75c7dd8fef54cc0339da41fd0d25a6ed2ef092d671310a4a6e25dc5cecbd` |
| Native compiler | Inno Setup 6.6.1；SHA-256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc` |
| 安装包 | `target/product-bundles/forme-windows-x86_64-20261028-fc4e23e-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| 安装包大小 | `73,294,667` bytes |
| 安装包 SHA-256 | `258a6e04823bc9782a045ae465ac95e86ae248c1b4229e7110b08f8c71f4abf6` |
| 签名/安装 | `not_performed` / `not_performed` |
| release ready | `false` |

`bundle-inventory.json`、`SHA256SUMS` 和 `native-build-receipt.json` 均绑定上述 release id、bundle digest 和源构建输入。构建过程未读取、打包或上传 owner 数据、`.forme`、模型配置或密钥；候选输出使用新目录，没有覆盖既有候选。

## 产品边界

- 对外是一个 ForMe 软件 appliance：一个 Product URL、一组系统服务、一个 Owner、一个 ProductionRuntime/主脑和一套持久数据；Web 是统一控制面，不是外部 Agent 的桥接页。
- Host、Web、Brain 和 worker 是同一实例内部的权限、故障和生命周期隔离单元；用户不需要认识内部地址、服务名或凭据。
- 安装后无有效 owner/模型配置时，Brain 可以保持停止；认领并保存有效配置后，Host 必须自动协调 Brain，正常流程不要求用户点击“启动主脑”。

## 当前安装现场边界

当前 `C:\Program Files\ForMe` / `C:\ProgramData\ForMe` 中的安装实例仍是旧候选 `forme-windows-x86_64-20261027-7cb5b8b`。本候选没有覆盖安装目录、替换服务程序、写入配置或改变服务状态；因此本报告不能证明当前安装已运行本候选，也不能证明配置保存后的真实 `BrainReady`。

只有在 owner 针对本候选批准一次受控升级后，才能验收以下闭合链：

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
$env:CARGO_BUILD_JOBS = '2'
cargo build --locked --release --workspace
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id forme-windows-x86_64-20261028-fc4e23e --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-fc4e23e
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-fc4e23e --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261028-fc4e23e-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

对当前已安装实例执行升级、替换程序、启动服务或读取受保护配置，需要针对本候选另行执行明确的 owner 操作授权；此前对其他候选的授权不自动覆盖本候选。
