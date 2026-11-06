# 原 Host 路径恢复实际结果（2026-11-06）

承接[固定计划](installed-original-host-path-restore-20261105-plan.md)。owner 在上轮列出两项待推进动作后指示“继续下一步”，本轮按同一计划、同一脚本摘要执行一次 `-Apply`，没有改写脚本或改用另一恢复路径。

## 实际动作与结果

- 固定脚本 SHA-256：`93defb96100806aac062a3298cadee9d098144b563f9ae657e38a7fd2a9d43bf`；在 Windows PowerShell 5.1 管理员进程执行，前置完整核验通过。
- 2026-11-06 11:38（Asia/Shanghai）仅恢复 `forme-host` 同一 image 的路径分隔符，将 `bin/forme-hostd.exe` 改为 `bin\forme-hostd.exe`；四服务账户/启动策略及其他服务登记保持。
- 原登记/策略/文件摘要/journal 水位已保存在计划指定的独立受保护目录。原六程序和六配置、Host policy 与备份 ACL/摘要终检通过，程序/config 写入均为零。
- 显式请求 Host start 一次，连续稳定后 Web start 一次；未启动 Brain/updater。最终原脚本 outcome 为 `original_host_web_restored_not_an_upgrade_or_ip_acceptance`，无失败阶段。
- 独立 SCM 复查：Host Running/PID 75976、Web Running/PID 32464；Brain/updater Stopped/PID 0。18765 listener 对应 Web PID；HTTP GET `/` 返回 200、`text/html`、595 字节，包含产品入口标识。

原始结果文件位于本工作树 `target/legacy-release-review/native-original-host-path-restore-apply-20261105.json`，SHA-256 为 `b867907db4fe4f5234d15db0ad178bf0f02ce5f10b21cf3338cc40dec3b4da28`。文件名中的 20261105 是原计划身份，实际动作时间在结果内，不另造一份历史执行。

## 验证限度与后续

这证明原 Host/Web 与 HTTP 页面入口已恢复；没有完成浏览器登录/真实模型/工具验证，没有替换 reader、迁移 journal、裁决历史 Upgrade、切换 active release、读取登录凭据或配置模型密钥。Brain 尚未运行，历史八类证据缺口及 IP14–15/IP24 继续独立闭合。

原 writer 已重新持锁，不从外部绕过锁读取活动 journal，不重复 Apply，不重派原 Upgrade。日志和历史记录保留，后续管理程序修复仍使用独立精确材料与执行计划。
