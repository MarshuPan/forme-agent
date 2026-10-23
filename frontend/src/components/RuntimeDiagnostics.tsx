import { useEffect, useState } from 'react';
import type { WorkbenchClient } from '../api/client.ts';
import type { RuntimeDoctorView, RuntimeView } from '../types.ts';
import { runtimeCheckLabel } from '../runtime-diagnostics.ts';

export function RuntimeDiagnostics({ api, runtime }: { api: WorkbenchClient; runtime: RuntimeView | null }) {
  const [view, setView] = useState<RuntimeDoctorView | null>(null);
  const [revision, setRevision] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    setView(null); setError(''); setLoading(Boolean(runtime));
    if (runtime) void api.runtimeDoctor().then((next) => {
      if (active) setView(next);
    }).catch((problem) => {
      if (active) setError(problem instanceof Error ? problem.message : '运行诊断暂不可读。');
    }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [api, runtime, revision]);
  function refresh() { setView(null); setError(''); setLoading(true); setRevision((value) => value + 1); }
  return <article className="surface" aria-busy={loading}>
    <div className="button-row"><h2>运行诊断</h2><button className="secondary-button" disabled={!runtime || loading} onClick={refresh}>刷新诊断</button></div>
    <p className="muted">检查已加载的配置与能力。模型、工具和消息渠道的实际连接需分别验证。</p>
    {!runtime ? <p className="notice" role="status">主脑离线，当前无法读取运行诊断。</p> : <>
      {loading && <p role="status">正在读取运行诊断…</p>}
      {error && <p className="form-error" role="alert">{error}</p>}
      {view && <><p className="status-pill">{view.blocking ? '存在阻断项' : '当前装配检查无阻断项'}</p>
        {!view.checks.length && <p className="muted">没有可显示的检查项。</p>}
        <dl>{view.checks.map((check) => <div key={check.name}><dt>{check.name}</dt><dd>{runtimeCheckLabel(check.status)}（{check.status}）{check.reason && <p className="muted small">{check.reason}</p>}</dd></div>)}</dl>
      </>}
    </>}
  </article>;
}
