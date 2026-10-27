import { useEffect, useState } from 'react';
import { WorkbenchClient } from '../api/client.ts';
import type { ProductConfigValidation, ProductConfigView } from '../types.ts';

/** Configuration writes are reconciled by Host; this view never asks for a second password. */
export function ConfigurationActivation({ api, config, coreConnected, onSaved, refresh }: {
  api: WorkbenchClient; config: ProductConfigView | null; coreConnected: boolean;
  onSaved: (value: ProductConfigView) => void; refresh: () => void;
}) {
  const [validation, setValidation] = useState<ProductConfigValidation | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState('');

  useEffect(() => { setValidation(null); setError(''); }, [config?.active_revision]);

  async function check() {
    if (!config || busy) return;
    setBusy(true); setError('');
    try { setValidation(await api.validateProductConfig(config.active_revision)); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '配置检查未完成。'); }
    finally { setBusy(false); }
  }

  async function reload() {
    if (busy) return;
    setBusy(true); setError('');
    try { onSaved(await api.productConfig()); refresh(); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '状态读取失败。'); }
    finally { setBusy(false); }
  }

  if (!config?.configured) return <p className="muted">保存模型配置后，后台会自动协调并启动主脑。</p>;
  const acknowledged = config.acknowledged_revision === config.active_revision;
  return <div className="configuration-activation">
    <h3>主脑生效状态</h3>
    <p className="muted">配置保存后由 Host 自动协调主脑服务。这里不会再次要求登录密码；手动启停只保留在维护页面，用于故障恢复。</p>
    <div className="button-row"><button type="button" className="secondary-button" onClick={() => void check()} disabled={busy}>检查已保存配置</button>
      <button type="button" className="secondary-button" disabled={busy} onClick={() => void reload()}>刷新生效状态</button></div>
    {validation && <p role="status" className="notice">{validation.valid ? '静态检查通过' : '静态检查未通过'}。模型连接尚未实测。</p>}
    {acknowledged && <p role="status">主脑已确认加载配置版本 {config.active_revision}。{coreConnected ? '当前主脑可连接。' : '当前主脑连接尚未确认。'}</p>}
    {!acknowledged && config.pending_restart && <p role="status" className="notice">配置版本 {config.active_revision} 已保存，后台正在启动或受控重启主脑；{coreConnected ? '主脑已连接，等待它确认配置。' : '主脑连接尚未确认，请稍后刷新。'}</p>}
    {!acknowledged && !config.pending_restart && <p role="status">配置已保存，等待主脑状态刷新。</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>;
}
