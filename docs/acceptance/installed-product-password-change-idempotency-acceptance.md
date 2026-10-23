# 改密码操作幂等绑定组件验收

状态：实现已提交；最终 Rust runtime tests 未验证，不作为通过组件或 IP 场景。

文档契约提交：`7e199a7`。实现提交：`dd76f20`，分支 `shu/installed-product`。未合入 main。

## 范围

- `/auth/password` 将 `Idempotency-Key` 传入独立认证库；schema v7 原子迁移至 v8，新增密码修改操作表，不改脑事件、brain store、crate 依赖图或 provider 配置。
- 操作记录绑定 owner、session、account ref、credential revision、auth epoch、目标 Argon2id salted hash 和 operation digest。同 key 同目标在候选有效期内复用原 hash/digest；异义复用、绑定变化、过期候选在 reauth 和 credential mutation 前拒绝。
- 认证库不存目标口令明文。候选 hash 最长保留 24 小时；启动或下一次改密请求会清理过期候选；成功提交在 credential/epoch/会话撤销的同一事务中清除该 owner 的所有候选 hash。最多 64 个待完成候选。
- Web 为新的明确意图生成新 key；连接结果未知时保留 key、将原字段设为只读并允许原请求继续核实。readonly 保留浏览器 `FormData` 字段，不会像 disabled 控件一样丢失重试 payload。
- PRD25 已补充该行为契约。嵌入式静态资源已从前端源重新构建。

## 验证

| 检查 | 结果 | 证据边界 |
|---|---|---|
| `cargo fmt --all -- --check` | PASS | 最终实现提交前执行。 |
| `cargo check -p forme-gateway --all-targets --locked` | PASS | 最终实现源码及测试 target 可编译。 |
| `cargo clippy -p forme-gateway --all-targets --locked -- -D warnings` | PASS | strict Clippy，最终实现源码。 |
| `npm run verify` | PASS | TypeScript、33/33 前端行为测试、嵌入式静态资源核对。 |
| `py -3 tools/code-quality.py` | PASS | 547 个文件，63 项既有规模债务未增长。 |
| `bash tools/compliance-doctor.sh` | PASS | upstream、license、copy 三门。 |
| `cargo test --workspace --all-targets --locked` | UNVERIFIABLE | 已尝试；链接阶段出现 Windows 页面文件不足、`No space left on device`。后续候选清理和职责拆分后，C 盘空间更低，最终源码未重跑 runtime tests。 |
| IP01–IP24 / 完整产品浏览器验收 | NOT_RUN | 本组件证据不升级为安装、浏览器或整机产品验收。 |

此前曾在职责拆分和最终毫秒窗口清理调整之前，对临时版本运行 `cargo test -p forme-gateway --lib password_change --locked`，4/4 通过。由于随后对候选 TTL、启动清理和职责边界有修改，该结果仅作历史诊断，不能代替最终提交上的测试。

## 未闭合

- 在恢复足够磁盘与 Windows pagefile 后，重跑最终源码 `cargo test --workspace --all-targets --locked` 及最终 Gateway 密码修改专项测试；在此之前不标记本组件 runtime test PASS。
- 本组件未验证真实安装服务、真实浏览器 password unknown 流、多进程争用或 IP06/IP21/IP24。
- 未删除 `target/verify-tests-20261023` 或其他构建目录；没有执行系统服务注册、真实账号/owner 数据迁移或读取密钥。
