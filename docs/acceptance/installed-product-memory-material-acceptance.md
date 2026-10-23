# 记忆来源材料正文读取组件验收

验证日期：2026-10-24。施工分支 `shu/installed-product`，实现提交 `de3c7b1`。本报告只记录记忆工作台的只读来源材料能力，不关闭完整 Web 工作台、原生安装或 IP01–IP24。

## 1. 实现范围

- 增加版本化产品适配对象 `WorkbenchMaterialQuery` / `WorkbenchMaterialView`，入口绑定当前 owner 可见的 `node + run + event + snapshot`，不接受浏览器自报路径、URL 或任意 `ContentRef`。
- Web `POST /api/v1/brain/workbench/material` 通过现有认证、签名桥和同一 `ProductionRuntime` 到达 core；不创建第二个 runtime，不直连模型、工具或脑数据库。
- Harness 在数据读取 fence 内重建 Memory 投影，核对列表摘要、节点、源 `MemoryNodeAppended` 事件、owner、scope，结束时复核 run heads。陈旧快照返回冲突，跨 owner、节点、事件、scope 或 schema 的请求拒绝。
- 实际运行产物和 owner 上传正文在 writer 侧登记加密 companion：`runtime-artifact-material-v1:{content_ref}`。正文、companion 和原有 metadata 属于同一生命周期主体；companion 不进入普通 artifact 下载、catalog 或 context hydration。
- 只返回完整、可验证的 UTF-8 纯文本，最大 65,536 bytes。状态明确区分 `Available`、`ReferenceOnly`、`MissingOrErased`、`TooLarge`、`NonText`；不返回截断片段，不执行 HTML/Markdown/SVG，不把正文当指令。
- 前端只在 owner 显式点击后读取，验证身份、来源、字节数和 SHA-256；正文只保存在当前组件内存，关闭、切换或登录代次变化后清除。

## 2. 验证结果

| 验证 | 结果 | 证据 |
|---|---|---|
| Harness 专项 | PASS，5/5 | `target/material-reader-runtime-20261024.log` |
| Web→core 正式 runtime 集成 | PASS，1/1 | `target/material-reader-web-integration-20261024.log` |
| 既有 artifact writer 回归 | PASS，1/1 | `target/material-reader-writer-20261024.log` |
| artifact catalog / lifecycle 回归 | PASS，5/5 | `target/material-reader-artifact-regression-20261024.log` |
| 前端 TypeScript、行为与资源构建 | PASS，53/53；静态资源 7 files / 70 locked dependencies | `target/material-reader-frontend-20261024.log` |
| 完整源码质量门 | PASS，160 suites、858 passed / 0 failed / 5 默认 ignored | `target/installed-product-quality-20261024-material-final.log` |

专项正反场景覆盖：登记正文及重启读取、节点/事件/run/snapshot/scope/schema 越权拒绝、内部 companion 冒充阻断、同形用户 JSON 保持普通资料、非 UTF-8 与超限无部分文本、生命周期删除后正文与 companion 不复活、重复读取不写脑事件/不调用模型/不创建新 key，以及 Web 登出期间的响应隔离。

## 3. 边界与未宣称事项

- 这是组件级源码与 loopback/fixture 集成证据，不是真实浏览器操作、真实 owner 数据或真实 provider 证据。
- 不因此宣布 IP17–IP24、完整 Web 工作台、安装包、系统服务、OS worker 隔离或三平台原生支持完成。
- 历史引用没有可信 writer companion 时保持 `ReferenceOnly`，系统不会猜测或从任意路径读取原文。
- 产品整体进度仍以 [独立部署与 Web 产品化实施记录](installed-product-implementation-progress.md) 和 IP01–IP24 策略为准。
