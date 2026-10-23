# 恢复候选工作台与已暂存发布认证组件验收

2026-10-23，`shu/installed-product@e36b5b9`。承接 prd/24 §4.10、prd/26 §4.3。它是整体交付中的组件记录，不能替代原生恢复、自更新、IP01–IP24 或最终当前源码的 M5/closure 验收。

## 实现与证据

恢复完成回执增加版本化安全候选投影，绑定最终 manifest、owner/store 和恢复执行边界；普通备份及只读验证不产生激活能力。工作台将生成候选与切换主脑分开，切换使用 Host 的配置/product 双 revision 和近期口令；丢响应只查原 operation，重新登录后也可从维护历史核实。Host 在停服前拒绝再次激活当前候选。

已暂存发布必须经唯一成功 staging operation、原始安装绑定、不可变 record 的 journal receipt、固定验签密钥和实际闭合文件集重新认证。返回 handle 不能从请求反序列化，使用前重新核对实际字节；兼容性使用观察到的 config/SQLite schema。当前没有 schema migrator 时拒绝需要新写入版本的发布。这是升级前置，尚未实现完整 Upgrade/Rollback/self-update。

| 层级 | 本批结果 |
|---|---|
| 完整源码门 | `tools/verify-code-quality.ps1` 退出 0：155 个 Rust suite，817 passed / 0 failed / 5 默认 ignored；fmt、all-target check、strict Clippy、前端 22/22、可重复嵌入资源、依赖审计、打包/真实 Rust 验签、NTFS 7/7、完整默认合规通过。 |
| 宿主门 | `tools/verify-installed-host.ps1` 退出 0，新增四个发布重新认证场景与所有既有命名场景通过。恢复阶段使用受控服务 adapter，不是原生 SCM。 |
| 实际进程 | `tools/verify-product-configuration.ps1` 退出 0：22 个 Rust 配置场景、当前 binary 构建及真实 hostd/webd/gatewayd/hostctl HTTP/管道链路通过。新增 Web→core 备份→准备→批准恢复→查询原回执，核对磁盘最终 manifest 摘要、活动路径不变、模型调用数不增加。模型为本地 fixture。 |
| 浏览器 | 临时独立安装登录后，从设置页已完成恢复回执进入独立确认；候选/摘要/配置和影响可见，缺原生能力时 checkbox、密码和切换按钮均禁用。截图位于本机 `target/installed-restore-candidate-web.png` 与 `target/installed-restore-candidate-controls.png`，未将临时数据作为发布资产。 |
| 规模/边界 | 最终结构门扫描 530 个文件、62 处既有规模债务未增长；18 crates/99 EventKinds 未改变。未读取真实 owner/model 密钥或注册本机服务。 |

交互预览进程的 stdin 未保持可写，截图后精确停止了该次临时进程；不将这次中断预览计作完整进程用例 PASS。完整无交互进程门已独立通过。预览生成的临时目录清理被自动审批策略阻断，目录保留；源码、截图和此前门的结果不受影响。

## 整体仍开放

恢复的原生 SCM 联调、完整签名 Upgrade/Rollback、自更新接管、旧脑/备份接管、强隔离 worker/桌面 helper、LAN/完整设置、对话附件完整性及最终核心/产品联合验收继续实施。当前成果未合入 main、未 push/发布，组件完成不作为任务终点。
