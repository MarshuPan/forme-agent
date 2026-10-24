# 模型角色与正式装配绑定组件验收

日期：2026-10-24。分支：`shu/installed-product`。设计 `087c1df`，实现 `700bb6f`。承接 architecture/09 §2.3 与 prd/25 §11.5.1；不提升原生安装或 IP08/IP21/IP24 的状态。

## 1. 行为与兼容

显式 foreground/cognition 路由进入正式 composition 的规范摘要。profile 清单不变但角色改变时摘要改变；同配置重启稳定。同 profile ID 的模型、provider、endpoint、能力、兼容性、fallback、费率或成本改变也影响摘要。credential 值、引用、环境槽位和 resolver 路径不参与摘要。角色均未配置时保持历史算法，未新增 manifest 字段、EventKind 或依赖边。

历史 SessionBound 不改写；默认审批和待决恢复继续核对既有 composition binding。受管测试验证角色变化、重启、凭据轮换、实际 HTTP 模型/凭据消费和零调用装配；该 fixture 没有可执行工具，不把它称为完整审批执行链验收。既有审批恢复否定专项另行验证 16 类不兼容上下文的零副作用。

## 2. 当前源码结果

- 完整 `tools/verify-code-quality.ps1`：PASS；161 suites，870 Rust passed / 0 failed / 5 默认 ignored。strict Clippy、前端 54/54、bundle 9/9、打包输入 15/15、实际 Rust 验签 5/5、临时 NTFS 7/7 与完整默认合规通过。
- `installed_brain_preferences` 3/3、models lib 17/17、默认审批恢复否定专项 1/1、冻结 workspace/99-kind/97-prefix 门通过。专项不重复累计到全量数字。
- 593 个受检源文件、63 处既有规模债务未增长，未刷新 baseline。质量门启动后未改源码。

本机证据目录：`target/installed-product-evidence/model-role-binding-20261024/`。

| 日志 | SHA-256 |
|---|---|
| code-quality.log | `87FE9124F607C459A17DFF8385345457733303924F4F67001A2865B52A8E9011` |
| managed-runtime.log | `54852D22A6FBFB0D5E5BDDC16356C8EC52B26E656077BE0C0273DA2EADC70C96` |
| approval-recovery.log | `84FF6B2461BE99152881F9B768011D7C9355674346ABC63230A1398B4CD69A9B` |
| frozen-contract.log | `8B5CF4FE819BCAAFAF83992136F7384DB1DBED0FBD545085873E8C42C79B95E3` |

## 3. 证据边界

未调用真实外部 provider、读取真实 owner/key、注册系统服务、合入 main 或推送；无新增浏览器证据。完整 M5 当前源码 release audit 和安装产品联合验收仍开放，继续按整体交付清单推进。
