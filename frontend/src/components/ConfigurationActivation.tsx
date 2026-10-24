import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { newKey, WorkbenchClient } from '../api/client.ts';
import { activationRejectedBeforeDispatch } from '../configuration.ts';
import type { ProductConfigValidation, ProductConfigView, ProductCoreActivation, ProductOperation } from '../types.ts';

/** Host receipts and actual core acknowledgements stay visibly separate. */
export function ConfigurationActivation({ api, config, coreConnected, onSaved, refresh }: {
  api: WorkbenchClient; config: ProductConfigView | null; coreConnected: boolean;
  onSaved: (value: ProductConfigView) => void; refresh: () => void;
}) {
  const [validation, setValidation] = useState<ProductConfigValidation | null>(null);
  const [operation, setOperation] = useState<ProductOperation | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const pending = useRef<ProductCoreActivation | null>(null);
  const [needsProbe, setNeedsProbe] = useState(false);
  useEffect(() => { setValidation(null); }, [config?.active_revision]);

  async function check() {
    if (!config || busy) return;
    setBusy(true); setError('');
    try { setValidation(await api.validateProductConfig(config.active_revision)); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '配置检查未完成。'); }
    finally { setBusy(false); }
  }
  async function activate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!config || busy || pending.current || validation?.revision !== config.active_revision || !validation.valid) return;
    const form = event.currentTarget;
    const password = String(new FormData(form).get('password') ?? '');
    pending.current = { schema_version: 1, operation_id: newKey('core-activate'), expected_revision: config.active_revision,
      action: coreConnected ? 'restart' : 'start', requested_deadline: Date.now() + 120_000 };
    setBusy(true); setError(''); setOperation(null);
    try {
      const result = await api.activateProductConfig(pending.current, password);
      setOperation(result.operation); refresh();
      if (result.operation.stage === 'committed' && result.operation.pending_stage === null) pending.current = null;
      else setNeedsProbe(true);
      try { onSaved(await api.productConfig()); }
      catch { setError('服务操作已返回原回执，配置状态刷新未完成，请重新读取。'); }
    } catch (problem) {
      // No automatic retry: the OS may already have received this operation.
      if (activationRejectedBeforeDispatch(problem)) { pending.current = null; setNeedsProbe(false); }
      else setNeedsProbe(true);
      setError(problem instanceof Error ? problem.message : '启动结果尚未确认。');
    } finally { form.reset(); setBusy(false); }
  }
  async function probe() {
    if (busy || !pending.current) return;
    setBusy(true); setError('');
    try {
      const result = await api.productOperation(pending.current.operation_id);
      setOperation(result); onSaved(await api.productConfig()); refresh();
      if (result.stage === 'committed' && result.pending_stage === null) { pending.current = null; setNeedsProbe(false); }
    } catch (problem) { setError(problem instanceof Error ? problem.message : '尚未核实原操作，请保留该操作标识。'); }
    finally { setBusy(false); }
  }
  if (!config?.configured) return <p className="muted">保存模型配置后，可在这里检查并启动主脑。</p>;
  return <div className="configuration-activation">
    <h3>检查并应用</h3>
    <p className="muted">静态检查确认配置和密钥文件可用；模型能否正常回答，需要主脑启动后用真实对话验证。</p>
    <div className="button-row"><button type="button" className="secondary-button" onClick={() => void check()} disabled={busy}>检查已保存配置</button>
      <button type="button" className="secondary-button" disabled={busy} onClick={async () => { try { onSaved(await api.productConfig()); refresh(); } catch (problem) { setError(problem instanceof Error ? problem.message : '状态读取失败。'); } }}>刷新生效状态</button></div>
    {validation && <p role="status" className="notice">{validation.valid ? '静态检查通过' : '静态检查未通过'}。模型连接尚未实测。</p>}
    {config.acknowledged_revision === config.active_revision && <p role="status">主脑已确认加载配置版本 {config.active_revision}。{coreConnected ? '当前主脑可连接。' : '当前主脑连接尚未确认。'}</p>}
    {validation?.valid && validation.revision === config.active_revision && !needsProbe && <form onSubmit={activate} aria-busy={busy}>
      <label>登录密码<input name="password" type="password" required autoComplete="current-password" maxLength={128} disabled={busy} /></label>
      <p className="muted">{coreConnected ? '重启前会停止接收新任务，并等待当前工作安全收尾。' : '只启动这台设备已安装的主脑服务。'}</p>
      <button className="primary-button" disabled={busy}>{busy ? '正在提交…' : coreConnected ? '应用配置并重启主脑' : '启动主脑'}</button>
    </form>}
    {needsProbe && <div className="notice"><p>原启动操作的结果需要核实。核实只查询记录，不会再次启停服务。</p><code>{pending.current?.operation_id}</code><button type="button" className="secondary-button" disabled={busy} onClick={() => void probe()}>核实原操作</button></div>}
    {operation && <p role="status">服务操作：{operation.stage === 'committed' ? '已完成；配置是否生效请以上方主脑确认为准。' : '尚未完成或需要恢复，请核实原操作。'}</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>;
}
