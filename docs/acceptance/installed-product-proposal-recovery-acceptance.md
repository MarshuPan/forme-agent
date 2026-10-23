# 普通主动建议处置的刷新恢复组件验收

日期：2026-10-23。施工分支：`shu/installed-product`；实现：`507cccf`；契约：`8f10bc5`、`038f801`。本报告只覆盖 PRD26 §4.6/§5.5 的普通建议恢复，不宣称 IP20/IP24 或全部安装产品通过。

## 1. 实现与边界

- Web identity schema v9 在签名 bridge 转发前保存 owner、原 command/payload、规范化绑定、run/proposal 与时间。命令全局唯一，owner/target 唯一，最多 256 条；不写脑表或新增 EventKind。
- `GET /api/v1/workbench/proposals/pending` 认证后只读原命令。读回核对闭合 schema、目标与 binding；损坏时整个读取失败，不部分显示。
- 相同 key/相同 body 可以显式核实，换 key 或改参数不能绕过未决目标。Unknown、503、错误回执和通信故障保留记录。绑定一致的终态或该建议入口确定的执行前 409 才条件清除。
- 收件箱先核对恢复队列，再开放普通建议操作；已从当前投影消失的建议仍保留原操作卡片。加载/刷新不自动转发，原表单锁定；浏览器不持久化恢复队列或凭据。
- 本机维护 session fence 接受 schema v9，撤销旧 cookie 不会丢失原 owner 的待核实命令。身份检查与队列 mutation 在同一个 SQLite IMMEDIATE 事务内。
- 责任拆在 identity/proposals 与 HTTP proposal controls，不扩大 session metadata 模块；冻结内部依赖图、99 EventKinds 与主脑处置语义保持。

## 2. 执行证据

当前实现源码已执行：

| 检查 | 结果 |
|---|---|
| fmt / workspace all-targets check / workspace all-targets test | PASS；157 个测试二进制，844 passed / 0 failed / 5 默认 ignored。 |
| strict Clippy | PASS。 |
| identity 恢复专项 | 5/5；精确幂等、异义/异 owner 拒绝、重启、容量、坏记录/回执、schema 升级和维护撤销。 |
| 受控 core 的真实 Web HTTP | PASS；读回不转发，Unknown 保留，换号/改参零转发，503/坏 binding 保留，最终 Confirmed 清除；5 次转发全为原 body。 |
| 正式签名 bridge / ProductionRuntime 命名门 | 8/8；含 pending GET 认证和真实 core 执行前 409，零处置/动作事件。 |
| 前端 strict TypeScript / 行为测试 / 静态资源 | PASS，40/40；7 个固定资源、70 个锁定依赖。 |
| 锁定依赖重装 / npm audit | PASS，未改变 package/lock；0 vulnerabilities。 |
| bundle / 原生包输入与 POSIX / 实际 Rust 验签 / NTFS | PASS；NTFS 7/7，仅临时组件。 |
| 结构棘轮 / 完整默认合规 / diff check | PASS；552 个源文件，63 处既有规模债务未增长。 |

统一质量脚本首次在 `npm ci` 因已运行 Vite 占用 Rolldown native module 而中断；此前 Rust 门已通过。依赖恢复后，将 npm 留下的被占用生成模块原样移到 `target/frontend-locked-module-20261023`，核对 SHA-256，保留已有预览进程；随后独立补跑同一脚本剩余的 npm ci/audit、前端、打包、签名、NTFS、完整合规与 diff 门，全部通过。没有把首次中断记为单次脚本全绿，也没有重复修改依赖锁。

本机可复查日志为 `target/installed-product-proposal-quality.log`、`target/installed-product-proposal-quality-tail.log`、`target/installed-product-proposal-workbench.log`。它们是可重建本机运行证据，不随源码打包。

## 3. 尚未证明

本组件没有单独验证真实浏览器刷新/双 tab、多个 Web 进程争用、原生服务重启或真实 provider；完整产品验收和当前最终源码 M5/release audit 仍按整体交付清单推进。受控 HTTP/单元证据不能替代这些结论。
