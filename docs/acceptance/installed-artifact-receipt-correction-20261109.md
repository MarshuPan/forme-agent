# 加密执行工件回执修复验收（2026-11-09）

本次为既有 `ArtifactStore` 行为修复，代码提交 `85afa70ccf2514f53a6897d0f47d50cac8ade9d9`，基于 `d089a7c22e8b3040afb65a9a22638d39b70dfb7c`。不修改冻结 trait/DTO、18-crate 图、99 EventKinds、原生边界或历史回执；不是新的 Final、安装升级或 IP01–IP24 完成声明。

## 1. 问题与结果

`ProtectedRuntimeArtifacts::write` 已用原始内容生成保护对象与材料登记，但返回 `ArtifactRecord` 时只替换了底层文件的 content_ref，沿用了密文文件的 digest/bytes。调用者取得的引用可解密为原始内容，摘要和长度却描述另一份密文。

本次返回原始内容的摘要和长度，保留保护写入、材料登记、密文存储与失败顺序。历史对象和已提交动作回执不重写；该提交也未部署到当前安装。

新增回归使用真实 SQLite、authority lease、AEAD 与工件 sink，覆盖自建二进制/文本、相同输入重复写入、解密结果与材料回执一致，以及底层文件不包含原文。二进制样本仅用于工件测试，不宣称是有效 PNG 或真实桌面截图。

## 2. 实际验证

| 检查 | 结果 |
| --- | --- |
| 修复前回归 | `artifact-receipt-before-fix-v2.log` 中原始摘要断言实际失败，exit 101。更早的 fixture 编译失败不作为缺陷证明。 |
| 相同回归修复后 | `artifact-receipt-after-fix.log`，1 passed，exit 0。 |
| 格式 / workspace check | exit 0。 |
| 完整 workspace test | 183 targets，1,311 passed / 0 failed / 16 默认 ignored，exit 0；ignored 不计通过。 |
| strict workspace Clippy | exit 0。 |
| D 波 RuntimeOnly | exit 0；不评估 C24 live，不代表 D 波完整验收或 Final。 |
| 结构质量门 | PASS，884 files，63 existing size debts。 |
| 完整 compliance | upstream / license / copy 均 PASS，exit 0。 |

原工作区验证在格式、check、test 全部结束后因剩余空间不足停止，整体原回执 exit 1 保留。对 49 个本工作树闲置测试程序做摘要/长度保持的可逆 NTFS 压缩后，仅继续 Clippy、D RuntimeOnly、质量与合规；没有重复完整工作区测试、删除缓存或降低 20 GiB 阈值。续跑前后校验原日志、源码和两份回执绑定，续跑最终 exit 0。

## 3. 原始工件

工件均在本工作树 `target/closure-inventory-evidence/`，不含密钥或模型正文：

| 工件 | 用途 / 摘要 |
| --- | --- |
| `artifact-receipt-checks-process.json` | 原格式/check/test 及空间停止记录；SHA-256 `b5fde9ac982cdd9117fde4f7117bd4382972e052d5a8f9f471dc4c2d856009a0`。 |
| `artifact-receipt-remaining-checks-process.json` | 续跑 exit 0、四阶段日志摘要与原回执摘要。 |
| `artifact-receipt-test.log` | 183 targets 的原工作区输出；SHA-256 `fd2a2253d6fbf861d23f99d57f4d9f008c5aa84d23b11e5e7e017846bac5d7e9`。 |
| `artifact-receipt-d-runtime.log` | D RuntimeOnly；SHA-256 `f5241df921b0abfd9f8a0c4918848d189cd464837eb8e188806bedc648963914`。 |
| `artifact-receipt-cache-compression.json` | 49 个缓存程序压缩前后摘要、长度与空间记录。 |
| `artifact-receipt-final-build-resources.json` | 固定 target 最终空间报告 PASS，无遍历错误，预算内。 |

两份过程回执共同绑定的源码 SHA-256：

- `crates/harness/src/production_artifact_store.rs`：`d6758220c48bcaa1eb20e2a0eab4746d750f94476287d0829be4e4b338a5544d`。
- `crates/harness/src/production_artifact_store/tests.rs`：`d38830fe407525bf2657a4cb49ba78ccc5e4b670c4d408c778bc8048568dd85f`。

## 4. 完成边界

本次只关闭回执逻辑摘要/长度错误。桌面授权、helper 结果交换、工件 Web 消费者、引用输入及真实 PTY 仍须分别实施和验收；[桌面结果接线提案](../architecture/proposals/installed-desktop-output-closure.md) 仍待明确批准，不因这项修复自动冻结。

原签名候选 `20261109-7b13529`、原生批次及 Unknown 请求未变。只读复核仍缺三份 correction-scope 管理员/原生/产品窗口回执，安装继续为 `20261107-cf3fbc9`。本次不从源码测试推出真实截图、原生升级或整体产品完成，剩余义务沿[现有清单](installed-product-remaining-obligations-audit-20261109.md)推进。
