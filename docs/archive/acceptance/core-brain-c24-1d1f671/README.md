# C24 首次真实模型试验：未通过的历史证据

源码 `1d1f671`，正式 runner 使用本机真实配置，每类两次，与相同事实输入的直接模型基线对照。独立校验返回 `Unverifiable`：冷启动、长期工作区、多代理、AskToLearn、失败重协调五类各两次通过；文件工具两次均实际写入成功，但后续模型请求失败，没有 final output。

排查发现，兼容模型请求缺少原始 assistant tool call 和对应的 tool result ID。此处保留修复前的三份原始脱敏生成文件，不能作为当前 D 波 PASS 或最终清单的一部分，也不能用后续成功结果抹去这一失败记录。修复及当前结论见正式 D 波验收报告。

历史 JSON 的结构、摘要与各类计数仍可由 `forme-core-brain-d-artifact-verify --require-pass` 检查；预期 exit 2。它绑定旧源码，不应通过当前源码一致性发布门。
