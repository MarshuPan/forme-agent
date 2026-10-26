# Installed Product Attachment Download Acceptance

日期：2026-10-26  
分支：`shu/installed-product`  
范围：真实 `forme-webd` 进程上的附件上传、查询、授权下载与响应头隔离。

## 结论

本专项通过。它证明 Web 附件下载经过认证的 session/scope 检查，并以不可执行的下载响应返回内容；不等同于完整安装器、原生系统服务、外部 Provider 或 IP01-IP24 整机验收。

## 已验证边界

- 4 MiB 附件经过真实 Web、签名 bridge 和 production content store；重复相同 command 幂等，改变 payload 被拒绝。
- 普通 JSON 控制请求仍受小体积上限约束；过大的附件请求被拒绝。
- HTML/SVG 内容下载时强制返回 `Content-Type: application/octet-stream`、`Content-Disposition: attachment` 和 `Cache-Control: no-store`，响应体与上传摘要一致。
- 文件名中的 `..\\..\\outside.svg` 不被当作服务器路径；路径样式文件名和原始 payload 不进入脑 store 明文。
- 错误 session、错误 scope 和伪造 companion content ref 均被拒绝。
- 附件维护路径不调用模型；重启恢复后，原附件仍可查询和下载。
- bridge response parser 接受固定长度、chunked 和 204 响应；拒绝重复 `Content-Disposition`、截断/歧义响应和非 identity 编码。

## 执行证据

```text
node --test tools/tests/managed_configuration.test.mjs
1 passed, 0 failed
duration_ms: 135513.9899

cargo test --locked -p forme-gateway --lib web::core::response::tests
2 passed, 0 failed
```

真实进程测试使用了本地 fixture provider 和临时 product 数据目录；未读取或上传真实 owner 数据、模型密钥或 provider 凭据。

## 对应实现

- `crates/gateway/src/web/core.rs`：bridge response 携带受控 `Content-Disposition`。
- `crates/gateway/src/web/core/response.rs`：解析并拒绝重复或非法处置头。
- `crates/gateway/src/web/http/core.rs`：只允许精确值 `attachment` 透传到 Web。
- `tools/tests/fixtures/artifacts_web_process.mjs`：恶意 HTML/SVG、路径样式文件名、scope/session 和存储隔离验收。
- `tools/tests/fixtures/managed_product.mjs`：支持读取原始响应头的真实进程夹具。

