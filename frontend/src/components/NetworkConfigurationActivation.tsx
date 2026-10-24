import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { newKey, WorkbenchClient } from '../api/client.ts';
import { activationRejectedBeforeDispatch } from '../configuration.ts';
import { configureNetworkCommand, maintenanceComplete } from '../maintenance.ts';
import type { MaintenanceCommand, MaintenanceSnapshot } from '../maintenance.ts';
import type { ProductConfigValidation, ProductConfigView, ProductOperation } from '../types.ts';

/** Applies the staged Web projection through the host maintenance boundary. */
export function NetworkConfigurationActivation({ api, config, onSaved, refresh }: {
  api: WorkbenchClient; config: ProductConfigView | null;
  onSaved: (value: ProductConfigView) => void; refresh: () => void;
}) {
  const [validation, setValidation] = useState<ProductConfigValidation | null>(null);
  const [availability, setAvailability] = useState<MaintenanceSnapshot | null>(null);
  const [operation, setOperation] = useState<ProductOperation | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const pending = useRef<MaintenanceCommand | null>(null);
  const [needsProbe, setNeedsProbe] = useState(false);
  useEffect(() => { setValidation(null); setAvailability(null); }, [config?.active_revision]);

  async function check() {
    if (!config || busy) return;
    setBusy(true); setError('');
    setValidation(null); setAvailability(null);
    try {
      const [report, snapshot] = await Promise.all([api.validateProductConfig(config.active_revision), api.maintenance()]);
      setValidation(report); setAvailability(snapshot);
    }
    catch (problem) { setError(problem instanceof Error ? problem.message : '网络配置检查未完成。'); }
    finally { setBusy(false); }
  }

  async function apply(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!config || !validation || busy || pending.current || config.network_active) return;
    const form = event.currentTarget;
    const password = String(new FormData(form).get('password') ?? '');
    setBusy(true); setError(''); setOperation(null);
    try {
      const view = await api.maintenance();
      pending.current = configureNetworkCommand(config, validation, view, newKey('network'), Date.now());
      const result = await api.maintain(pending.current, password);
      setOperation(result);
      if (maintenanceComplete(result)) {
        pending.current = null; setNeedsProbe(false);
        onSaved(await api.productConfig()); refresh();
      } else setNeedsProbe(true);
    } catch (problem) {
      if (!pending.current || activationRejectedBeforeDispatch(problem)) { pending.current = null; setNeedsProbe(false); }
      else setNeedsProbe(true);
      setError(problem instanceof Error ? problem.message : '网络配置应用结果尚未确认。');
    } finally { form.reset(); setBusy(false); }
  }

  async function probe() {
    if (busy || !pending.current) return;
    setBusy(true); setError('');
    try {
      const result = await api.maintenanceOperation(pending.current.operation_id);
      setOperation(result);
      if (maintenanceComplete(result)) {
        pending.current = null; setNeedsProbe(false);
        onSaved(await api.productConfig()); refresh();
      }
    } catch (problem) { setError(problem instanceof Error ? problem.message : '原网络操作尚未核实。'); }
    finally { setBusy(false); }
  }

  if (!config?.configured || (config.network_active && !needsProbe && !operation)) return null;
  const supported = availability?.host.allowed_operations.includes('configure_lan') ?? false;
  const candidate = `${config.network.tls ? 'https' : 'http'}://${config.network.allowed_hosts[0] ?? config.network.bind}`;
  return <div className="configuration-activation network-activation">
    <h3>应用 Web 网络入口</h3>
    <p className="muted">保存不切换网络。应用请求先返回操作号，再由后台切换 Web；当前页面可能断开，不会自动重发。LAN 不降级为裸 HTTP。</p>
    <p>候选入口：<code>{candidate}</code>。发布状态不能证明入口可达或证书受信任。</p>
    <div className="button-row"><button type="button" className="secondary-button" onClick={() => void check()} disabled={busy || Boolean(pending.current)}>检查网络配置</button></div>
    {validation && <p role="status" className="notice">{validation.valid ? '静态检查通过。' : '静态检查未通过。'}{supported ? '设备支持提交此切换；入口健康和跨设备连通性仍需核实。' : '系统服务或防火墙核验尚未就绪，不能提交此切换；LAN 规则须由管理员预先配置。'}</p>}
    {validation?.valid && supported && validation.revision === config.active_revision && !config.network_active && !needsProbe && <form onSubmit={apply} aria-busy={busy}>
      <label>登录密码<input name="password" type="password" required autoComplete="current-password" maxLength={128} disabled={busy} /></label>
      <button className="primary-button" disabled={busy}>{busy ? '正在应用…' : '应用网络配置并重启 Web'}</button>
    </form>}
    {needsProbe && <div className="notice"><p>原网络应用操作结果需要核实，不会自动重试。切换后可在候选入口登录，查看“设备维护”的近期记录并核实同一操作号。</p><code>{pending.current?.operation_id}</code><button type="button" className="secondary-button" disabled={busy} onClick={() => void probe()}>核实原操作</button></div>}
    {operation && <p role="status">网络操作：{maintenanceComplete(operation) ? '已完成。' : '尚未完成或需要恢复，请核实原操作。'}</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>;
}
