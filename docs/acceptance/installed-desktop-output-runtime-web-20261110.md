# 安装版截图运行时与认证读取组件验证

本记录承接 owner 已批准的截图输出方案及正式契约。原生传输/保护投影实现为 `656f16a`，认证 Web 读取为 `0a254164e3ea0ed097e634da05ea0ad37d8e6aa2`。本记录不关闭安装版截图原生联合验收、Host 执行域副本删除、视觉模型消费者或受管 MCP/插件/渠道义务。

## 运行时已接通

Windows helper v2 沿原认证 named pipe 传输精确绑定原 job/permit/grant/plan 的 PNG。签名发布中 helper 与输出支持声明共同验证，旧 peer 在 Started 前拒绝截图；原 request 共用单调期限，部分进展不延长。Host 完整核验长度、摘要、PNG、当前授权与会话后封存，核心通过正式 ArtifactStore 装配写入受保护存储，先取得材料登记再完成。响应丢失及投影中断只重取原结果，不能重新截图。

`656f16a` 对应 workspace 191 targets、1,440 passed / 0 failed / 22 默认 ignored；fmt/check、strict Clippy、installed-host、质量和完整合规通过。原 D RuntimeOnly 的 73 项全部通过，但最后 seal 因工作树未提交而引用旧 HEAD 失败，原 FAIL 保留。提交后在干净 `656f16a` 重新执行 D RuntimeOnly，73 项通过并成功 seal，独立 verifier 通过。这里不声明完整 D/Final 或原生 IP 验收。

## 认证 Web 原件读取

目录由原 RunAccepted/SessionBound、ActionPlanned/Started/Completed、原 ExternalActionReceipt 与受保护材料登记解析。缺登记、pending、Unknown、已删除对象不提供预览。目录仅读取受限元数据；每次分块读取持 data fence，重查 owner/session/run/scope/digest/删除状态，并独立核验解密原件和 PNG。

Web 仅新增有限 v2 目录/读取入口；内部 core assertion、原认证会话及响应绑定逐次核验。PNG 分块最多 256 KiB，响应为 image/png、nosniff、no-store。现有 core fence 将已准入读取排在注销提交之前；注销提交后新请求 401，core 调用数不增长。前端完整核对范围、长度、SHA-256 和尺寸后才创建内存 Blob URL；关闭、取消、隐藏、换会话/run/工作面和认证失效均回收。点击退出时立即卸载图片消费者，不等待注销 HTTP 返回。模型仍只有引用/状态，没有像素输入。

## 认证读取的最终源码验证

`desktop-output-web-final3-gates.json` 的十阶段实际 exit 0，UTC 2026-11-09 15:51:35–16:08:12。1,020 个源码条目、日志、计数与资源原件在提交前独立核验一致。

| 检查 | 结果 |
| --- | --- |
| fmt、workspace all-targets check/test、strict Clippy | PASS；192 targets，1,450 passed / 0 failed / 22 默认 ignored |
| 前端 TypeScript、完整行为测试、内嵌资源一致性 | PASS；131 tests |
| 正式 Web 工作台组件门 | PASS；8 个命名场景，真实 loopback/runtime，模型为自建计数夹具 |
| code-quality、完整 compliance-doctor | PASS；988 files，63 项既存规模债务 |
| 空间报告 | PASS；固定 target 逻辑 61,189,998,733 bytes，去重分配 58,040,501,969 bytes，低于 64 GiB 预算；未清空缓存 |

以下原件均在本工作树 `target/product-integration-evidence`，敏感安装凭据不入仓库。

| 原件 | SHA-256 |
| --- | --- |
| `desktop-output-runtime-gates.json` | `9b527ac7a8d6ff24a3fdfa1a1d4185e64873a3c5e2a622040972f30d229941f0` |
| `desktop-output-runtime-clean-d/runtime.json` | `6364673774e76c906ebfc4cb2adcb7f89876d238c2b2b0f6a10d57ea15308d4b` |
| `desktop-output-web-final-gates.json` | `4fe632758fe3387db9dc9a2d35826edc94de9f169a6575f5dca86069f9bbd179` |
| `desktop-output-web-final2-gates.json` | `05affceed71fe863df124a88f00d4f155bc18949f0d06670676bfd6f8a44c14e` |
| `desktop-output-web-final3-gates.json` | `75e887dac3c57e1edfc6d83c59559952824fcb9daf0ee16008f21ca6317e92c8` |
| `desktop-output-web-final3-independent-verification.json` | `f0a940d7eca0b263f4abf5c13b3373d87c435cda4178f39b317b82c7e2913892` |
| `desktop-output-web-browser-final.json` | `6edd71ce0c0ff76e11a45d4ec452a26348951a92209aa50d9484c0de6dfbd212` |
| `desktop-output-web-browser-rejections.json` | `6efc194ed8a9fbe830ea3a5ce30c30c5838671c0b2414988591ed4fe23619c95` |
| `desktop-output-installed-preflight.json` | `456af14e55bb835f2e8c84ddc71b27d0f314a2f2e612620abd1edf468d7989c9` |

## 提交后 D 波运行时核验

干净提交 `0a254164e3ea0ed097e634da05ea0ad37d8e6aa2` 的 D RuntimeOnly 在 UTC 2026-11-09 16:14 完成，73/73 场景通过、seal 成功；独立 `runtime_report` 再核对当前 inventory 与 source digest，结果 PASS。这不是完整 D/Final 或安装版原生验收。

| 原件 | SHA-256 |
| --- | --- |
| `desktop-output-web-clean-d/runtime.json` | `03f8b176aa999f74f6fc5914d78ecce1737aebe98fd9c8a143ef9cb7e6760bc2` |
| `desktop-output-web-clean-d-independent.json` | `ae1aae6923e4868c176d25d9d69466d2109e8575de106bd7822a5e67b1a70dee` |
| `desktop-output-web-clean-d.log` | `92c4a397aa0157b42c51904cef7d1fb4e08cc1a1e37aeba937d1687956ab06e1` |

## 真实浏览器组件

通过 Ego Lite 操作正式 ToolArtifacts、WorkbenchClient 和读取模块，自建 HTTP 夹具提供测试 PNG，未连接安装服务或截取用户桌面。浏览器实际解码 512×256、405,447 bytes 的 PNG，读取 Blob 后独立重算摘要 `sha256:95776bddf60877e911b025cbe88662faceb34ae376de35c5e8ce976531e462e9` 与原图一致。换外部会话、内容损坏、删除、慢速取消和退出均验证；累计创建 4 个 Blob URL，退出后 4 个全部 revoke，页面无图片。TaskSpace 已结束，夹具服务已停止。

Page.captureScreenshot 两次超时，未产生截图文件；上述证据是实际 DOM 解码、字节及交互状态记录，不声称有视觉截图工件。

## 保留的失败与修复

HTTP 并发测试最初错误地同步等待注销后才释放核心读取，与既有 core fence 形成等待循环；已改为验证真实线性化顺序，没有弱化生产锁。另保留 PowerShell 5 native stderr 驱动错误与 Rust 测试 closure 借用编译错误。

第一次整仓在 Harness 并行测试中出现 7 项临时目录 AlreadyExists；原因是同一个 fixture 文件被两个模块加载，各自持有相同计数器。随后 strict Clippy 的 duplicate_mod 明确拒绝重复加载。最终只定义一个共享测试模块、两组测试共同导入，删除测试同时核验正文存在/消失；相关 28 项通过，最终整仓结果另列。

## 现场与后续

2026-11-09 23:58（Asia/Shanghai）只读实测本机仍为 `forme-windows-x86_64-20261109-36f8662-dev`，Host/Web/Brain Running，product/core Ready，revision/ack 3，issue_refs 为空。本轮新实现尚未部署。

源码复核另发现安装 placement 使用 `surface:windows-default`，原生截图驱动只接受 `surface:monitor:<编号>`；这是下一项必须修复的生产接缝，不能用本组件证据宣称原生截图成功。需按原授权 bounds 映射当前唯一显示器，拒绝越界、跨屏或歧义，不扩大为全屏。

完整 helper→Host AEAD→core AEAD→实际浏览器的一次原生 capture、原身份/授权/会话负例和 Host 副本删除仍待执行。首次安装的受信输出支持也仍需接通；本次已实现签名活动发布的支持绑定。原输入、PTY、其他平台和完整 IP01–IP24 义务不因上述组件通过关闭。
