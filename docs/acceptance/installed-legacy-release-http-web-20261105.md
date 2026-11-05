# 历史升级审查的正式 HTTP/Web 接线（2026-11-05）

承接 owner 已批准的[历史审查协议](../architecture/proposals/installed-legacy-release-recovery-review.md)和[Host/journal 组件验收](installed-legacy-release-review-20261105.md)。实现提交为 `ad05bc7`，施工分支为 `shu/bootstrap-claim-journal`。本批仅实现并验证源码通路；未替换已安装程序、迁移现场 journal、启停服务或重跑原 Upgrade，不是安装现场的版本声明。

## 1. 实现和故障处理

- 五个 Host 路由只在 Configuration 域分派；公开 Web 对应五个显式 allowlisted 路由，不代理管理通配路径。匿名请求、错误 Origin/CSRF、伪造字段和错误 key 在 Host IO 前拒绝。
- Web 从真实 owner session 重建 owner；apply 与 verify 分别绑定完整规范化摘要进行密码重新认证。Host DTO、产品 journal 和回执均不接收密码。报告只由 Host 提供，浏览器不上传证据、路径、公钥或成功断言。
- Web/浏览器验证回执 schema、owner、operation、request digest、报告 binding、期限和 revision。原申请被替换或回执多出字段时拒绝；不因能解析 JSON 就判定操作完成。
- 新增独立历史审查面板，保留普通恢复面板。刷新/断线只 GET 原号和原报告；Intent 只在用户明确确认、重新认证后走独立 verify，未知结果不自动换号或重发。
- 修复 Intent 后原报告无法重读的边缘问题：只读返回 journal 保存的同一已签报告，原请求过期不改变报告期限。查询不调用 collector、不追加事实、不推进操作；报告过期仍拒绝，verify 仍重新取得实际观察。首次 apply 的严格原 checkpoint 检查保持。
- 浏览器按 Rust struct 字段顺序计算 domain digest，HTTP JSON 属性重排不改变摘要；共同 golden 覆盖 binding、report 与原 request。owner/API/维护水位变化后屏蔽迟到页面响应。

生产仍没有挂载真实原生 collector、独立受保护信任配置及管理 reader 修复；执行请求继续拒绝。这是已接线但受真实前提约束的通路，不是可强制通过的恢复开关。18 crates、99 个脑 EventKinds、冻结依赖图与五个 unsafe 边界保持。

## 2. 当前验证

| 层次 | 证据 | 结果 |
|---|---|---|
| 正式公开 HTTP/真实登录 + 内部 loopback transport | Gateway 历史审查 3 项测试；真实 owner/session、CSRF、Origin、口令、closed DTO、精确转发与跨 owner/篡改回执拒绝 | 3 passed / 0 failed。内部 Host 响应由原创受控服务提供，不证明原生材料可信。 |
| 前端 | strict TypeScript；完整行为测试；可重复静态资源构建 | PASS；105 passed / 0 failed，含历史审查新增 5 项。 |
| 全工作区源码门 | `tools/verify-code-quality.ps1` | PASS；180 Rust targets，1174 passed / 0 failed / 15 默认 ignored；fmt/check/strict Clippy、结构门（818 files、63 处既有规模债务未增长）、前端 105/105、bundle 10/10、原生包/POSIX 18/18、签名 5/5、NTFS/防火墙只读策略、npm audit 和完整合规均通过。Inno 退出夹具未提供工具链，仍为 NOT_RUN。 |
| 永久宿主门 | `tools/verify-installed-host.ps1` | 272 个命名场景全部 PASS；末尾明确 IP01–IP24 NOT_RUN。 |
| 适用核心 closure | `tools/verify-core-brain-closure.ps1 -Wave A/B/C`，D 使用 `-RuntimeOnly` | A/B/C 均有最终 PASS。D 在获准清理旧缓存后重新完整执行，退出码 0、最终 runtime-only PASS；日志为 `http-web-closure-d-verified-20261105.log`。不证明新的 C24 live、D/Final 或 IP 验收。 |
| 当前 M5 | `tools/verify-m5.ps1` 与当前树审计回执 | 本批尚待重验；上批回执不能证明 `ad05bc7` 的当前树，不能沿用旧的完整门结论。 |
| 真实浏览器、已安装服务和迁移 | 原生 collector/信任与 reader 修复、真实材料、独立本机方案 | 未验收；未对已安装实例执行本批代码。 |

测试目录及日志位于本工作树 `target/legacy-release-review/`：最终源码门为 `http-web-quality-verified-20261105.log`，宿主门为 `http-web-host-20261105.log`，closure 为 `http-web-closure-{a,b,c,d}-20261105.log`。早一轮测试将未知字段 Host 回执错误预期为 503，实际是现有 transport 的 502；已修正断言并完整重跑上述最终源码门。请求转发后的摘要/绑定不可信由 Web 返回 503 `LegacyRecoveryUnverified`，反序列化拒绝仍为 502 `ConfigurationRejected`；两者都保持结果未知、不换号重发，不能把早一轮失败日志改写成通过。

测试口令/签名为原创 fixture；日志不记录真实账号密码、模型 key 或用户正文。浏览器 client 的 GET/POST 测试不能代替实际 DOM 交互/截图或产品 IP 验收。实际只读安装观察另见[原生预检](installed-legacy-release-native-preflight-20261105.md)，不与 fixture 合并计算。

上轮已结束的 D 进程日志停在 C24 runner，缺少最终 PASS；恢复执行时空间门为 LOW_SPACE（16.5 GiB < 20 GiB），在 Cargo 前拒绝。owner 随后明确批准仅清理旧 `harness-control-maintenance/forme/target` 的 dev 缓存；仓库工具重查无占用，删除 33,798 个可重建文件，实际回收 `69,453,881,344` bytes。清理前后 933 个源码文件、5 个运行/研究文件、41 个保留 target 文件的摘要及旧工作树 HEAD/status 均一致。原报告保存在 `approved-old-dev-cache-clean-20261105.json`；没有删除当前工作树缓存或安装证据，没有放宽资源阈值或缩减测试。清理后的新 D 日志是本次通过证据，旧未完成日志保持原状。

## 3. 后续依赖

继续完成真实原生收集器、独立信任/签名管理 reader 修复、确切本机迁移方案及批准后的原生验证；八类历史证据缺项仍为 Unverifiable。旧脑接管的实际发现/解密/数据 worker/账号事务与 active binding，真实 provider、完整浏览器工作台及 IP01–IP24 联合证据按各自报告推进。本批不宣告整体目标完成。
