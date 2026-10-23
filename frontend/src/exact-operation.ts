/** One unresolved mutation owns its original key and value until confirmed.
 * Read/refresh never calls dispatch. A 409 is released only by explicit reload.
 */
export interface ExactRequest<T> { key: string; payload: T }
export class ExactOperation<T> {
  private original: ExactRequest<T> | null = null;
  busy = false;
  conflict = false;
  get pending(): boolean { return this.original !== null; }
  get key(): string | null { return this.original?.key ?? null; }
  async run(create: () => ExactRequest<T> | Promise<ExactRequest<T>>, dispatch: (request: ExactRequest<T>) => Promise<unknown>): Promise<void> {
    if (this.busy) throw new Error('原操作仍在提交，请等待原回执。');
    if (this.conflict) throw new Error('操作发生冲突，请先重新读取权威状态。');
    this.busy = true;
    try {
      this.original ??= structuredClone(await create());
      await dispatch(structuredClone(this.original));
      this.original = null;
    } catch (problem) {
      this.conflict = typeof problem === 'object' && problem !== null && 'status' in problem && problem.status === 409;
      throw problem;
    } finally { this.busy = false; }
  }
  releaseConflict(): void {
    if (this.busy || !this.conflict) throw new Error('结果仍不明，必须核实原命令。');
    this.original = null;
    this.conflict = false;
  }
}
