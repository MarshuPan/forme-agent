import type { RunSummary } from './types.ts';

export function runStatusMessage(summary: RunSummary): string {
  if (summary.result && typeof summary.result === 'object' && !Array.isArray(summary.result)
      && summary.result.stop_reason === 'model_call_outcome_unknown_after_restart') {
    return '服务重启时模型调用尚无完整回执；回复和费用均无法确认，forme 没有自动重试。你可以在核对后另开一次运行。';
  }
  if (summary.status === 'Waiting') return '任务正在等待输入、审批或结果核实。';
  if (['Complete', 'Aborted', 'Failed', 'Limited'].includes(summary.status)) {
    return '本次运行没有可展示的最终回答。';
  }
  return '主脑正在处理，公开进度见任务详情。';
}
