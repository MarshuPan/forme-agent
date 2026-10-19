# D 波产品任务证据

`product-golden.json`、`score.json` 和 `report.txt` 来自正式 `forme-core-brain-d-golden`。生成提交、配置摘要、证据等级、六类重复次数和计分保留在工件中；`runtime.json` 与 `manifest.json` 另外绑定 D 波 70 项逐名执行及封闭文件清单。只有真实配置、各类至少两次、独立 selector 与当前源码一致性校验全部通过，才构成 C24 有界任务通过。

```powershell
cargo run -p forme-eval --bin forme-core-brain-d-artifact-verify -- docs/acceptance/core-brain-d-artifacts/product-golden.json --require-pass
# Pass 返回 0，Unverifiable 返回 2，Fail/非法工件返回 1。
```

校验器独立核对 schema、内容 digest、固定任务/selector、完整 stream 位置见证和实际模型引用。受控 HTTP 的实现测试不能将该工件升级为 LiveConfigured。看见 `hard_invariants=true` 或零 token/费用时，应同时查看试验次数；零次试验不表示完成任务或完成免费调用。

按 `../../runbooks/v1-core-brain-data-and-evidence.md` 在新的目录运行 `--live --repeats 2`。先核对独立校验结果、源码对应关系和敏感信息，再替换当前三份生成文件并提交。runner 拒绝覆盖已有目录；不要把凭证、endpoint、owner DB 或 `.content-keys` 放进本目录。

首次真实试验在 `1d1f671` 下暴露原生工具消息续接缺陷，五类通过、工具类没有最终回答。原始未通过记录保留在 `../../archive/acceptance/core-brain-c24-1d1f671/`，后续修复不回写该历史。

直接模型基线只比较相同事实输入的答案质量；文件工具、重启、多个 child、学习候选和失败重协调另用工作流证据核验。这套有界任务不证明数月使用收益，也不替代独立 memory/activation/orchestration/model-routing 对照。
