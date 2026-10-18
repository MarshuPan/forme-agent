# D 波产品任务证据

当前 `product-golden.json`、`score.json` 和 `report.txt` 来自正式 `forme-core-brain-d-golden`，源码为 `8589918b7dd8876788160bede591b1f2761d3ed4`。生成时没有可用的模型配置，因此等级为 **Unavailable**，六类任务均为零次实际试验，结论为 **Unverifiable**。这不是六类任务通过，也不是线上模型、长期价值或 V1 发布证明。

```powershell
cargo run -p forme-eval --bin forme-core-brain-d-artifact-verify -- docs/acceptance/core-brain-d-artifacts/product-golden.json --require-pass
# 当前预期 exit 2：Unverifiable。
```

校验器独立核对 schema、内容 digest、固定任务/selector、完整 stream 位置见证和实际模型引用。受控 HTTP 的实现测试不能将该工件升级为 LiveConfigured。看见 `hard_invariants=true` 或零 token/费用时，应同时查看试验次数；零次试验不表示完成任务或完成免费调用。

真实配置准备好后，按 `../../runbooks/v1-core-brain-data-and-evidence.md` 在新的目录运行 `--live --repeats 2`。先核对独立校验结果、源码对应关系和敏感信息，再替换当前三份生成文件并提交。runner 拒绝覆盖已有目录；不要把凭证、endpoint、owner DB 或 `.content-keys` 放进本目录。

直接模型基线只比较相同事实输入的答案质量；文件工具、重启、多个 child、学习候选和失败重协调另用工作流证据核验。这套有界任务不证明数月使用收益，也不替代独立 memory/activation/orchestration/model-routing 对照。
