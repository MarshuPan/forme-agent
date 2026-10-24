import type { ProductBrainPreferences, ProductModelProfilePatch } from '../types.ts';

export function BrainPreferences({ value, models, disabled, onChange }: {
  value: ProductBrainPreferences; models: ProductModelProfilePatch[]; disabled: boolean;
  onChange: (value: ProductBrainPreferences) => void;
}) {
  function route(role: 'foreground' | 'cognition', profile: string) {
    onChange({ ...value, model_routing: { ...value.model_routing, [role]: profile || null } });
  }
  return <fieldset className="model-profile" disabled={disabled}>
    <legend>工作空间与模型分工</legend>
    <p className="muted">默认工作空间用于后续未单独绑定的运行；已有会话和项目保留各自的工作空间。</p>
    <label>默认工作空间标识<input value={value.default_workspace} required maxLength={128}
      pattern="[A-Za-z0-9_.:\-]+" onChange={(event) => onChange({ ...value, default_workspace: event.target.value })} /></label>
    <div className="form-grid model-form-grid">
      <label>对话与任务优先模型<select value={value.model_routing.foreground ?? ''} onChange={(event) => route('foreground', event.target.value)}>
        {value.model_routing.foreground && !models.some((model) => model.id === value.model_routing.foreground) && <option value={value.model_routing.foreground}>原模型已不在列表，请重新选择</option>}
        <option value="">自动选择兼容模型</option>{models.map((model) => <option key={model.id} value={model.id}>{model.model || model.id} · {model.id}</option>)}
      </select></label>
      <label>后台认知模型<select value={value.model_routing.cognition ?? ''} onChange={(event) => route('cognition', event.target.value)}>
        {value.model_routing.cognition && !models.some((model) => model.id === value.model_routing.cognition) && <option value={value.model_routing.cognition}>原模型已不在列表，请重新选择</option>}
        <option value="">使用默认模型</option>{models.map((model) => <option key={model.id} value={model.id}>{model.model || model.id} · {model.id}</option>)}
      </select></label>
    </div>
    <p className="muted">前台优先项仍需满足任务的能力条件。后台设置用于相关性判断；保存不会调用模型。</p>
  </fieldset>;
}
