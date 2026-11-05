# 工作空间约束与项目命令的前端协议验收

2026-11-06，分支 `shu/bootstrap-claim-journal`，实现 `088254d`，基线 `7b4c6f4`。承接 PRD23 的 C13 可执行约束及 PRD26 §4.7、§5.3；没有更改冻结 DTO、trait、EventKind、crate 依赖或原生权限。此记录只闭合发现的组件问题，不表示安装实例恢复或 IP01–IP24 通过。

## 1. 真实浏览器发现与修正

在临时 Host/Web/ProductionRuntime 中，原页面把目标引用、约束和禁止规则展示为没有说明的自由文本。自然语言约束被 core 正确拒绝，但页面只有通用错误；合法规则也无法保存，因为 `prepareCharter` 将版本号位于 `charter` 内的既有 `WorkspaceCharterChangedPayload` 当作外层带版本对象解析。

同一问题影响项目登记、修订和取消：这些既有成功响应分别为 `{event}`、`{event}`、`{cancelled:true}`，前端通用版本检查会在服务端完成动作后报错。先前进程测试直接用 HTTP helper 读取 JSON，绕过了浏览器客户端，因而没有发现此错误。

本次按既有 wire 形状解析响应。约束准备结果核对 nested schema、workspace、owner actor、完整请求内容、expected/committed version 及 digest 形状；不匹配则不发 PUT。摘要由 core 生成和最终验证，前端格式检查不冒充密码学验证。项目回执按操作核验；响应若显式包含不支持的版本仍拒绝。通用端点的版本检查、session generation fence、CSRF、原 body/key、Unknown 和 CAS 保持。

编辑器说明哪些字段是已登记引用、哪些填写项目自然语言说明；可执行规则显示既有语法和逐行错误，保留至少一条、最多 64 条、单项 512 bytes、范围包含关系和节点上限等输入检查。它们只提供提前反馈，core 的规则解析、最终出口约束及权限治理继续是权威。

## 2. 自动与真实进程验证

统一门 `pwsh -NoProfile -File tools/verify-code-quality.ps1` exit 0：182 Rust targets、1252 passed、0 failed、15 默认 ignored；fmt、locked workspace check/build/test、strict Clippy、结构/规模棘轮、依赖审计、前端 115/115、资源可重复构建、签名及包装边界、NTFS/防火墙只读组件和完整默认合规通过。861 个受检文件，63 项既有规模债务未增长。未传 Inno 工具链的原生退出夹具为 NOT_RUN；组件门不改变其状态。

最终增加未知外层版本拒绝检查后重新生成嵌入资源，并补跑 locked workspace all-target check、Gateway all-target test（7 targets、135 passed、0 failed、3 默认 ignored）和实际 daemon build。上述数字分别记录，不与全 workspace 统计重复累加。本次没有无变更地重跑 M5 或更新发布回执。

`node --test tools/tests/managed_configuration.test.mjs` 最终 exit 0。项目进程 fixture 改为经真正的 `WorkbenchClient` 消费实际 HTTP 响应，覆盖约束准备/提交/读回、项目登记、暂停、路线规划、后续段修订、陈旧版本拒绝和路线取消；主脑重启后再次核对约束、项目、暂停版本和路线状态。新增前端负例覆盖错 workspace、内容/actor 替换、版本错配、坏摘要、未知 schema 和退出登录后的迟到响应，失败不发后续 PUT。

第一次改造 fixture 的 HTTP adapter 时重复拼入大小写不同的 CSRF header，进程检查失败；已修正 header 转接，保留失败日志和独立新夹具的最终成功日志。这不是原生产品事故，也不把该失败计为通过。

## 3. 最终资源的浏览器验证

使用 computer-use 的浏览器接口操作自建临时账号；公开夹具凭据未写入报告。真实页面加载 `index-XD82LHEN.js`，最终资源 manifest SHA-256 为 `5601eeaf84771519d801a2b5525aef2e5c250a20a2de92c9de025247c5ad6691`。

实际操作：自然语言规则出现明确逐行错误；改为 `read-only`、`require-review` 后保存成功；登记 `project:browser-final-20261106`，暂停后续监管并修订项目说明，取消已存在的未执行路线。刷新后约束仍在，项目说明保留，监管 version=1/enabled=false，路线显示已取消且执行入口禁用。没有把取消请求等同于已证明在途副作用停止。该夹具没有派发真实工具或访问安装实例。

另完成文字认知纠正和刷新后原命令回执读回的浏览器检查；原候选降级，新反馈仍是候选。最终 daemon 夹具的纠正/真实 core 重启检查也 exit 0。模型为本地计数响应 fixture，不能据此声称真实 provider 或长期学习质量通过。临时进程经 stdin 正常结束，夹具执行既有清理；原安装、数据和凭据未修改。

本地证据目录为本工作树的 `target/web-browser-closure-20261106/`，未随公开源码提交；截图和日志没有真实账号、cookie、模型 key 或安装配置正文。

| 工件 | SHA-256 |
|---|---|
| `source-quality.log` | `94354be97ed5ea4237b86ed52a0f547165b9f2dd13903abc1011f7f17f907b0b` |
| `managed-configuration-final.log` | `b67e0d0eb6944495afd15aa417a5e2be4595e214a5271d7acf7eea491c7b6ca4` |
| `frontend-final.log` | `257e137d3ddd2da0469e0d3bf6e86d84a27026d13bb4dbe9bd97783a6d4cfd03` |
| `project-final-after-reload.jpg` | `5b53b8f56ccff45bcf600f8b6b31fd12b06410acc7fcb58c63a5d7a2775b470d` |

最终空间报告为同目录 `build-resources-final.json`：44,571,269,871 logical / 41,689,185,724 allocated bytes，低于 64 GiB 预算，`over_budget=[]`、`errors=[]`。固定 target 复用；没有清理其他缓存、安装包或原件。

## 4. 本机与总目标仍未闭合

结束时原 `forme-host/web/brain/release-updater` 四服务仍为 Stopped，产品地址 `127.0.0.1:18765` 未恢复。本轮没有 UAC 子进程、SCM 变更或原生启动。先前只读预检被 Windows 取消，不能报告为仍在等待或已通过。

[原 Host 路径恢复计划](installed-original-host-path-restore-20261105-plan.md)已可审查，但新增 PathName 修正的精确 Apply 确认尚未收到。此前的一次启动已失败，不使用本轮浏览器或源码证据替代该原生结果。[历史恢复审查](../architecture/proposals/installed-legacy-release-recovery-review.md)的生产 collector、独立来源连续性、管理修复及现场迁移仍未完成；不能给现有报告倒填来源或补签而宣称 Verified。完整原生安装、恢复、真实模型与 IP01–IP24 继续开放，总目标保持未完成。
