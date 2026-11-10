# 原 job 只读核实组件（2026-11-10）

本批实现已批准的“Unknown 只核实原 job”边界。首次 MCP 发现派发前保存完整原 WorkerDispatchRequest；显式核实只读该 job 的 probe/manifest/chunks，不重建 permit、刷新期限、领取 nonce、再次 dispatch 或 cancel。实现提交为 `b78b276e38afe6287e479f84083158e2f3a55d19`；文档细化在 `5de480b` 独立提交。

## 行为

- 生产 WorkerActionBackend 与 owner discovery 共用同一实例。记录器先核对私有 owner/run/session/command/spec/composition/worker 原件、原计划、已批准解析和 ActionPlanned，再加密不可变地保存原请求。保存失败零 ActionStarted/dispatch。原件与原 run/scope 服从同一删除 fence。
- 新认证入口 POST `/v1/managed/mcp/discovery/{run}/verify` 使用封闭版本化 command 正文；Web bridge 绑定原命令，保留认证、CSRF 和 8192 字节限额，不接收 job、permit、进程、环境或新授权。`publish` 仍只补发布，不读取或执行 worker。
- 核实锁住原会话，验证当前配置、policy、composition、worker 和原件，再读取原 job。只有完整 Completed 观察、原 permit/manifest/正文、保护材料、原事件链以及 Host 当前性全部成立才 CAS 追加一次 ActionCompleted。读取过期结果不续期授权；不完整、损坏、撤销或来源漂移保持 Unknown。
- 历史 Unknown 只在 Started 之后、可信 Completed 之前且存在完全一致的派发原件时可接受；Completed 后的 Unknown、远端 lease 或错误 job 提示仍拒绝。再次发布已完成目录也重新检查来源，不用旧 RunComplete 掩盖矛盾事件。
- 通用重启恢复不会在“Unknown 后已有完成事件、目录尚未发布”时抢先追加 RunComplete。专用路径完成发布和原验证要求后才收敛运行；并发/重复核实不重复完成、发布或真实执行。
- 原 Unknown 的能力结果记录保留，不将它改写成学习成功。生产能力门仍记录实际 L0，恢复零模型调用；普通工具原能力门不变。

## 验证与边界

新增九项 Harness 事件/持久化场景与一项 Web bridge 场景，扩展真实 HTTP 认证/CSRF/版本/字段/正文限制检查至 verify 路由。组件使用真实加密 SQLite、审批 broker、EvidenceCompetenceGate 与计数 worker transport，不代表真实 Brain Service SID 正向执行。旧来源、材料保护、worker 和其他产品回归沿完整工作区检查。

首次定向编译捕获跨 crate 私有 matcher 调用，已改为复用原 PolicyEngine；后续四项断言曾把原 Unknown 的负向结果记录误当新增成功，已精确断言保留唯一 unknown、无新增学习成功，不删除产品事实。第一轮完整检查止于 strict Clippy 的 match 结构警告，修正后使用新前缀重新验证，保留原失败日志。

原始材料前缀为 `target/product-integration-evidence/original-job-*`。fmt、workspace/all-targets check/test、strict Clippy、code-quality 与完整 compliance 均 exit 0。整仓 **200 targets、1,568 passed / 0 failed / 35 默认 ignored**；新增十项场景逐名通过，1,103 份源码摘要与日志原件独立核验一致。提交后在同一干净源码上另跑 A 波装配门，47/47 通过、exit 0，随后独立核对当前场景清单、源码绑定与封存摘要。没有部署、推送、UAC、服务变更或真实 owner 数据操作。目录在新 run 的工具快照、Host 独立 placement/公开 enabled、设置 UI、插件/渠道与 IP01–IP24 继续开放，原三项及窄发现补充的批准持续有效。


独立计数时发现 PowerShell 合并 stdout/stderr 会将下一个 Cargo Running 标题放到前一个 libtest 结果之前；已保留原解析器，改用 libtest 自身的开始/结果配对，并排除有明确嵌套记录的一项子进程 fixture。原日志和测试没有修改，不把子进程的同一项再计入整仓总数。

| 原件（本工作树 target/product-integration-evidence/） | SHA-256 |
| --- | --- |
| original-job-verified-r2-gates.json | 45f7caa842f521035517cd69592b5b1692140c74e96e3539affad23031156ebc |
| original-job-source-independent.json | 5a6722fde6bb92cfba07fb929263f83b5246c9205a11a1e618e125856176a723 |
| original-job-clean-a-gates.json | 24a22344bef026bef345c73fc492a2b848150cd4211072dd58cefd5a6872df6a |
| original-job-clean-a-independent.json | 61ac08dd629cf6e91355db39a3a954364a7e4dc8a0dac4650e728fcca495094b |
| original-job-verified-r2-test.log | fd6622e155f92eea06cae7642699c4a71962fb2c59263ab206d29855bdc2cb3e |
| original-job-verified-r2-build-resources.json | 63bb8e1e2b0719151be4cecb219c355e6d7fb7971bc24666835fc383a5c7b2ec |

完整 M5、更新后的发布审计与任务结束空间报告在 [M5 记录](m5-acceptance-report.md) 单独记录；本批提交前缓存实际分配为 70,557,901,752 bytes（约 65.71 GiB），高于原 64 GiB 报告预算，工具 PASS 仅表示统计完整、错误为空。后续回收须取新的测量值，不沿用上一批压缩数据。
