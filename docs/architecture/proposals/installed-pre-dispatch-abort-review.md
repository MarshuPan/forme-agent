# 未进入 updater 原生副作用的升级终止协议（待确认）

状态：**提案，未实施，未授权本机操作**。

本提案处理当前 `maintenance:upgrade-current-20261113-fa24bee` 这类现场：原升级已经落下 `Validated / pending Quiescing` 和 handoff，但当前没有该 operation 的 updater dispatch claim，updater 本次开机也没有启动。普通 `AbortBeforeSwitch` 仍要求升级前可信源基线；本提案不把“没有 claim”或“服务仍在运行”当作源基线，也不修改普通恢复的缺基线拒绝。

## 1. 适用范围

只允许已认证 owner 对已经存在的原 operation 明确发起一次终止，且同时满足：

- operation、原 command digest、owner、installation、expected revision 和 original release 全部精确绑定；只接受原号，禁止替换升级请求或生成新 Upgrade。
- journal 当前仍为 `Validated`、pending `Quiescing`，从未出现 `BackedUp`、`Switching`、`Checking`、`RecoveryRequired` 或已完成的恢复 intent。
- 受保护 host-state 中没有该 operation 的 dispatch claim；固定 updater 状态为 Stopped，且本次开机没有该 operation 的启动/失败记录。
- Host/Web/Brain 的进程身份、启动时间、活动 release、配置 revision、worker 状态和原 handoff 与原 operation 绑定一致；实际 active release 未改变。
- 当前 owner、配置和 journal revision 仍通过 CAS；原 journal、handoff、旧 target、失败记录和其他历史事实全部保留。

任何一项无法从受保护原件、SCM/固定 updater 观察和原 journal 复核得到，结果为 `Unverifiable`，不追加裁决事实。

## 2. 精确裁决

新增独立的 `PreDispatchAbortRequest/Intent/Committed`（schema、domain 和 digest 待协议冻结），字段至少绑定原 operation、原 command digest、owner、expected revision、观察摘要、一次性 idempotency key 和 ≤120 秒的 owner 核实窗口。它不是 `ProductReleaseRecoveryCandidate`，不生成或伪造 `ProductReleaseRecoveryBaseline`。

Host 在原 product journal 的唯一写者临界区中再次重查全部条件，然后以一个原子 committed fact：

1. 将原 operation 置为 `Aborted`，清除 `pending Quiescing`；
2. 追加独立的 `PreDispatchAbortCommitted` 回执；
3. 保留原 intent、takeover、handoff、失败引用、目标暂存内容和全部 journal hash。

回执只表示“在已核实的边界内没有允许 updater 继续 dispatch”；不表示升级成功、源数据连续性已证明、备份已恢复或产品已经 Ready。updater 后续若读取该 handoff，必须因原 operation 已 `Aborted` 而拒绝进入 claim/stop/backup/switch。

## 3. 拒绝与副作用边界

- 不停止或启动 Host/Web/Brain/updater，不删除 handoff/claim，不回滚或清理 target，不访问模型、脑库、worker 数据，不发脑事件。
- 已有 claim、服务启动时间漂移、任意原生 stop/backup/switch/checking 迹象、active release 漂移、worker Unknown/Pending、journal 锁不可取得、回执写入结果不明，均拒绝；不重试副作用。
- 同 key 同 body 只查询原回执；同 key 改参数、换 operation、普通恢复/维护 key 冲突均拒绝。连接中断保持 Unknown，只能按原 operation 查询。
- 该协议不能用于已进入 `BackedUp` 之后的操作；那些操作继续使用普通恢复或独立历史审查。

## 4. 需要先冻结的文档变更

实施前必须在 `canonical-contract.md` §29、`prd/24` §4.10、architecture 的 journal/fact 表和对应协议 DTO 中补齐：schema/version floor、digest domain、replay/fold、CAS、updater 读取顺序、HTTP/Web re-auth、拒绝码及事件序列测试。实现不得以现有 `AbortBeforeSwitch` candidate 缺省字段、浏览器自报或当前内存投影替代这些定义。

## 5. 当前现场判断

已取得的只读证据支持“尚未观察到该 operation 的 updater dispatch claim”，但尚未取得活动 Host 的独占 journal replay，也没有冻结本提案的协议对象。因此当前仍是 `Unverifiable / recovery_required`；本文件不是本机执行授权。

