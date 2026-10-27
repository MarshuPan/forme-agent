# Windows Host 诊断候选

2026-10-27，在 `shu/installed-product` 的源码提交 `694b6bba98e9a89119001bf5ce12f717ce9da5eb` 上重新构建。此候选用于把脱敏 Host 启动阶段诊断带入后续受控验证；它没有部署到现有安装，也不改变任何 IP01–IP24 状态。

## 构建证据

| 项目 | 结果 |
|---|---|
| 目标 | Windows x86_64；本机原生 release 编译 |
| Rust 构建 | `cargo build --release --locked --workspace`，PASS |
| Bundle | `target/product-bundles/forme-windows-x86_64-20261027-694b6bb/`，18 个文件，含六个程序、Web 资源与许可文件 |
| Bundle digest | `f52d46b8a9ee1bcd3a4dc7012f4951e9775865348b7cc95af67023bfa493aa9b` |
| Native compiler | Inno Setup 6.6.1；SHA-256 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc` |
| 安装包 | `target/product-bundles/forme-windows-x86_64-20261027-694b6bb-package/artifacts/forme-0.0.1-windows-x86_64.exe` |
| 安装包大小 | 73,222,084 bytes |
| 安装包 SHA-256 | `906713301d8076ec71566214b1d512d0db9e0373bad0af4a464c1a5242b381e` |
| 签名/安装 | `NotSigned` / `not_performed` |
| release ready | `false` |

构建回执位于 `target/product-bundles/forme-windows-x86_64-20261027-694b6bb-package/native-build-receipt.json`。候选目录与此前的 `7cb5b8b` 候选完全分离，未覆盖旧包。

## 诊断范围

源码已包含 Host 的有界、脱敏启动诊断：失败时只记录固定错误码、阶段和摘要引用，不记录错误正文、凭据、配置内容或私有路径。旧安装使用的是 `7cb5b8b` 候选，因此旧的 Host 退出 `1` 只能证明启动失败，不能提供该阶段信息。

## 当前边界

- 现有 `C:\Program Files\ForMe`、`C:\ProgramData\ForMe` 和四个 SCM 服务仍保持原状态；没有用新候选覆盖它们。
- 新候选是未签名开发包，不能直接绕过同一安装的 signed staging、active-release 和 updater 协议。
- 不能用直接替换程序文件、重跑首次安装器或新增临时服务名来代替受控 release 切换。
- 需要进入现有 SCM 诊断前，必须取得同一安装的正式签名 staging，或另行明确批准并记录仅用于诊断的开发替换路径；后者不能作为产品验收证据。
