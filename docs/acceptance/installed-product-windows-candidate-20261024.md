# Windows 原生安装候选构建验收

2026-10-24，在 `shu/installed-product` 上基于实现提交 `a53557f` 构建。目标是更新当前 Web 工作台对应的 Windows x86_64 安装候选输入；这是打包组件证据，不是 IP01 或 IP24 验收。

## 构建证据

- 主机：Windows 11 专业版，版本 `10.0.26200`，x64。当前 shell 非管理员。
- `cargo build --release --locked --workspace`：PASS。
- `tools/build-product-bundle.mjs`：PASS，目标 `windows-x86_64`，17 个文件，development=true，内容摘要 `d293726823f42ff2b68bf616dc4c3a60ddb6bff363b32785630a2674d4784ada`。
- `tools/build-native-package.mjs`：PASS，使用 SHA-256 固定为 `f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc` 的 Inno Setup 6.6.1 编译器。
- 候选 `forme-0.0.1-windows-x86_64.exe`：62,264,548 bytes；SHA-256 `804f27edc8c2ff9865a0d83698323f9e5d9468d7bd97b4b1f289c4145211c16f`。receipt 的 source bundle digest 与上述内容摘要一致，编译退出码为 0。
- 独立 `Get-AuthenticodeSignature` 结果为 `NotSigned`；receipt 标记 `native_package_signature=not_performed`、`native_installation=not_performed`、`release_ready=false`。

## 未覆盖

候选没有运行或安装，没有创建、启动或移除系统服务，也没有读取用户 owner/model 数据。当前 shell 非管理员，且本机没有已注册的 forme 服务。缺少正式签名包、安装后 SCM/身份/ACL 探测与反向断言，因此 IP01、IP02、IP03、IP10、IP24 仍没有新的 PASS 证据；其他操作系统/架构也未验证。

此候选仅保留在 ignored `target/` 构建目录，不是发布物；不能推送、安装或宣传为可用版本。
