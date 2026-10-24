# 原子文字纠正组件验收

日期：2026-10-24。分支：`shu/installed-product`。设计补充 `626e908`、`96e1a36`，实现 `802e3d2`；承接 prd/26 §5.6.3。此组件不关闭完整 IP20/IP24 或原生产品验收。

## 1. 实现与已验证行为

浏览器只提交原 run、对象种类/标识、来源事件、当前列表摘要、原命令号和纠正文字。认证 owner、scope、置信度、稳定性和派生列表由正式 ProductionRuntime 解析；伪造 actor/scope/confidence、错误 owner、陈旧快照、无效来源、fixed/constitutional 和非法文本均在写入前拒绝。

原候选降级；原稳定对象沿既有 RetractionEvent 与传递 ReevaluationTaskCreated 停用。反馈形成独立应用层密文来源、Working/0.4 的 Frame 判断候选及明确冲突，不直接改 UserModel、AgentSelfModel、Trust 或权限。最多两个 stream 的 CAS、来源密文及材料登记、原对象失效、新候选和无正文回执在同一 SQLite 事务内提交。实际 SQLite 后段故障证明全部回滚；同快照并发只允许一次提交。

同号同义返回原回执，异义复用冲突；Unknown 只核实原命令，不生成替代号。回执历史不保存反馈正文。真正删除反馈来源后，候选正文及派生内容清理，原失效事实和无正文回执保留；重启读取不会使旧错误恢复生效。

## 2. 当前源码验证

- `tools/verify-code-quality.ps1`：PASS，163 suites、878 Rust passed / 0 failed / 5 默认 ignored。strict Clippy、前端 64/64、bundle 9/9、打包 15/15、实际 Rust 验签 5/5、临时 NTFS 7/7 与完整默认合规通过。
- `tools/verify-workbench-correction.ps1`：PASS，Store 原子事务 3/3、Harness 4/4、正式 Web→core 命名场景 1/1、独立进程与重启 1/1。专项覆盖在全量中的测试不重复累计。
- `tools/verify-core-brain-workspace-contract.ps1`：PASS，18 crates、99 EventKinds、97-kind strict prefix 与 additive 兼容不变。
- 最终结构门：607 个受检文件、63 处历史规模债务未增长；fmt、diff 检查通过，未刷新 baseline。

全量 Rust 测试之后补充了长 ID 换行和复选框样式；最新静态资源已重建，随后前端 64/64、strict Clippy、专项 Gateway 与实际进程门通过。这里不声称全门期间所有文件完全固定。

本机证据目录：`target/installed-product-evidence/cognition-correction-20261024/`。

| 文件 | SHA-256 |
|---|---|
| code-quality.log | `07E0C908E907B2B023BD2637B0D3BB4118AD489590F8064FFAC3AF0C8B74D9D4` |
| correction-gate.log | `212CF72AA590B778DD604B1F646F6ABA533312F33CDE1D56344F97CDBE63956F` |
| frozen-contract.log | `344FB5DE2A676B19E301C467F82F3618BDF13FAF4E4BEDD0C05911E2F0B99C86` |
| correction-browser.png | `C10F1907B0602C1EB062CF2F39A7F5E9538EB4F436BD866271BD96F94D7F30AC` |
| correction-layout.png | `1E832B643EB002F85D8C4F86A8F8CB272DFA7E4B7697C90C342792D4C6FB0418` |

## 3. 浏览器证据与限制

真实浏览器在临时 host/web/core 实例完成登录、文字纠正、旧候选降级/新候选出现、刷新与原号只读核实。含 script 标签的反馈保持纯文字。首轮发现长引用横向溢出，修正后复验 viewport/content 均为 1265 像素，认知区域没有新增 script 元素；最新截图为 correction-layout.png。未执行真实窄屏、双 tab、多 Web 进程及整包联合验收，不能由 CSS 规则推断这些结果。

模型使用本机自建 HTTP fixture，不是真实 provider。正常独立进程测试与专项门通过；首个手动浏览器预览超过其 600 秒测试上限，临时 daemon 已由测试清理，预览的 stdin 等待未自行收敛。该手动预览超时独立记录，不改写为正式组件测试通过或真实模型证据。

未读取真实 owner 数据、模型密钥或 `.forme`，未注册系统服务、合入 main 或推送。用户授权清理的 `target/verify-tests-20261023` 已不存在，本轮无需删除；此前针对临时预览进程与目录的组合清理被自动审批拒绝，未执行，未据此扩大删除范围。

完整 M5/release audit、能力/渠道/网络配置、隔离执行、升级回滚和原生 IP 义务继续按整体交付清单推进，本组件不是总里程碑终点。
