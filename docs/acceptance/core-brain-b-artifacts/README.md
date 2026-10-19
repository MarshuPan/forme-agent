# B 波相关性对照工件

`runtime.json` 是当前源码 B 波全部逐名场景的执行记录；`tick.json` 补充三相 tick 的连续事件位置、范围、phase 顺序、认知/report 摘要及实际调用计数。tick 自身零模型调用与 owner turn 的真实端点调用分别记录，不能用自报计数代替。`manifest.json` 同时绑定这两份记录及下述相关性工件，校验命令为 `tools/core_brain_evidence.py --verify-wave B --require-pass`。

`relevance-comparison.json` 是 C11 的受控 HTTP 对照记录，不是真实线上 provider 或长期价值证明。标签由独立 fixture 给定；模型选择本身不是正确标签。该用例刻意保留负结果：同一快照、候选集、任务与 K 下，扩散选错、直接模型选对，运行时因此推荐 `DirectModel`。

验证命令：

```powershell
cargo run -p forme-eval --bin forme-core-brain-b-artifact-verify -- docs/acceptance/core-brain-b-artifacts/relevance-comparison.json
```

验证器重算 precision/recall/error、候选集合与 K 约束、策略推荐及内容 digest；篡改、越界、未知字段和未来 schema 不通过。工件中的 latency 是该次实测值，token/费用根据夹具返回的 usage 与已绑定 profile 计价，不外推线上质量或价格。

可通过 `forme-harness / v1_core_brain_b_activation` 的 `c11_common_snapshot_provider_baseline_records_honest_metrics_and_negative_result` 测试并设置 `FORME_CORE_B_ARTIFACT_DIR` 生成新记录。时间、call ID 与延迟可能变化，不要求两次工件逐字相等。永久 B 波门运行实现测试并独立校验仓库记录。
