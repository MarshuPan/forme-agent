# Windows 本机开发签名与原生安装器证据

日期：2026-10-30。本文只记录本机开发证书对 Windows 安装器的签名证据，不授权安装、升级、服务停止、文件替换、配置写入或 Brain 启动。`local-dev` 证书不是正式发布信任根。

## 1. 候选

本次候选由当前 `main` 的产品二进制重新构建；签名工具实现随后在 `3cd7e5d` 固化。原生编译使用已审查并固定摘要的 Inno Setup 工具链。

| 项目 | 结果 |
|---|---|
| bundle | `target/product-bundles/forme-windows-x86_64-20261030-d4be9ac` |
| native package | `target/product-bundles/forme-windows-x86_64-20261030-d4be9ac-package` |
| unsigned installer | `artifacts/forme-0.0.1-windows-x86_64.exe` |
| unsigned bytes / SHA-256 | `73,576,276` / `sha256:82f525f510be89457a0aa539d02fee6f5b2e8d9024227f4d8a1d265c8119a19a` |
| local signed installer | `artifacts/forme-0.0.1-windows-x86_64-local-dev-signed-v3.exe` |
| signed bytes / SHA-256 | `73,578,152` / `sha256:ab533e2daf9be744e32546aa1c5c3603255d09551b20f48f50a5b0bc1593f9eb` |
| signed receipt | `artifacts/forme-0.0.1-windows-x86_64-local-dev-signed-v3.exe.signing-receipt.json` |

源安装器和 signed 副本是两个不同文件；源文件保持 unsigned，未被覆盖。多个候选的回执按 signed 文件名绑定，工具拒绝覆盖既有输出或回执。

## 2. Authenticode 结果

| 项目 | 实测结果 |
|---|---|
| signature type | `Authenticode` |
| signer thumbprint | `84F1194BDE2A64769B52D04E5E9A2B7149605D5E` |
| subject | `CN=ForMe Local Development Package Signing` |
| store | `CurrentUser\\My` |
| signature status | `UnknownError` |
| trust status | `unverifiable` |
| Windows message | `A certificate chain processed, but terminated in a root certificate which is not trusted by the trust provider` |
| native package signature | `authenticode_present_unverifiable` |
| native installation | `not_performed` |
| release ready | `false` |

签名私钥留在当前用户证书存储，未进入仓库、bundle、安装器或回执。当前没有把本地根证书写入 Windows 信任根；因此 `UnknownError` 被如实保留为 `unverifiable`，不能关闭正式签名门，也不能作为升级准入。

## 3. 验证

- `node --test tools/tests/windows_package_signature.test.mjs`：`4 passed / 0 failed`，包含本机实际证书签名、输出回执和摘要核对。
- `tools/sign-windows-package.mjs` 使用固定 Windows PowerShell 5.1 模块路径，显式绑定证书指纹，独立读取 `Get-AuthenticodeSignature`，不接受签名动作状态与独立观察状态不一致的结果。
- `Get-AuthenticodeSignature`：unsigned 源为 `NotSigned`；signed 副本为 `Authenticode / UnknownError`，签名者指纹匹配。
- 安装/升级：未执行。当前运行中的 Host/Web、现有安装目录、服务注册、配置、密钥和 owner 数据均未修改。

这份证据只关闭“本机开发签名工具和一次 signed candidate 生成”的边界；不关闭 IP01–IP24、原生安装、服务身份、升级/回滚、BrainReady 或正式发布信任。
