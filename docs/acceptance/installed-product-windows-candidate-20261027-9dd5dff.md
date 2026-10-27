# Windows 端口修复候选

2026-10-27，在 `shu/installed-product` 当前源码提交 `9dd5dff1a899d54f4075018edc4a1dd83fdb7934` 上重新构建。该提交包含 `4bc4eecb37a7a2cc2e14e69e959826f6c8ee173d` 的内部 loopback 端口探测与持久化修复。本候选只证明可重复构建和离线包完整性，尚未安装、签名、注册服务或进入 IP01-IP24 验收。

## 构建证据

| 项目 | 结果 |
|---|---|
| 目标 | Windows x86_64；本机原生 release 编译 |
| 资源预检 | `py -3 tools/build_resources.py check` PASS；187.6 GiB free |
| Rust 构建 | `cargo build --release --locked --workspace` PASS |
| Bundle | `target/product-bundles/forme-windows-x86_64-20261027-9dd5dff/`；18 个文件，含六个程序、Web 资源与许可文件 |
| Bundle content digest | `74e8e3890750704085096c109e60031e41b0c7e50cc0e541e6adbdbfbf3ee370` |
| 修复后 Host | 39,407,105 bytes；SHA-256 `c842fc81ccedfc2e292ca02ab258c9b0e4ac8e38c9f18e3511cfb52c05c0acc4` |
| Native compiler | Inno Setup 6.6.1；SHA-256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc` |
| 安装包 | `target/product-bundles/forme-windows-x86_64-20261027-9dd5dff-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| 安装包大小 | 73,242,669 bytes |
| 安装包 SHA-256 | `d2cefc093c00c87dee93c7ae2e098c750c958c919fc1b2d95984904c35e3223e` |
| 签名/安装 | `NotSigned` / `not_performed` |
| release ready | `false` |

构建回执位于 `target/product-bundles/forme-windows-x86_64-20261027-9dd5dff-package/native-build-receipt.json`。bundle 使用 `verifyBundle` 以规范化绝对路径独立复核，18 个文件、每项摘要、Windows PE 头、闭合集、`SHA256SUMS` 和内容摘要均通过。首次直接传入未规范化的 Windows 路径被校验器拒绝，未修改验证器或候选文件。

## 修复内容与边界

- 安装准备阶段从受控的 `18766-29999` 范围探测可绑定的 loopback 内部端口。
- Host、AgentExecution、Configuration 和 Core 使用同一组已持久化端口；Web 对外默认端口 `8765` 保持不变。
- 本候选没有读取、打包或上传 owner 数据、`.forme`、模型配置或密钥。
- 本候选是开发包，未签名，`release_ready=false`；构建成功不等于安装成功。
- 当前安装仍是旧端口配置和已替换的诊断 Host 现场。新候选尚未覆盖当前安装，不能据此宣称 Host/Web、认领、真实模型、升级、恢复或 IP01-IP24 通过。

## 可复现命令

```powershell
py -3 tools/build_resources.py check
cargo build --release --locked --workspace
node tools/build-product-bundle.mjs --target windows-x86_64 --release-id forme-windows-x86_64-20261027-9dd5dff --binary-dir C:\code\forme\target\release --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261027-9dd5dff --development
node tools/build-native-package.mjs --bundle C:\code\forme\target\product-bundles\forme-windows-x86_64-20261027-9dd5dff --output C:\code\forme\target\product-bundles\forme-windows-x86_64-20261027-9dd5dff-package --toolchain C:\code\forme\target\native-toolchain-95b4b6a39c9a48d39819f6a262a07a3c\toolchain.json --version 0.0.1 --allow-development
```

实际安装、替换现有 Host 或启动服务需要针对这个新候选另行确认；此前对 `694b6bb` 候选的诊断授权不覆盖它。
