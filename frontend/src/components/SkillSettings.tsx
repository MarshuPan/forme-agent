import type { ProductSkillTrust } from '../types.ts';
import { emptySkill, skillTrustLabel, utf8Bytes } from '../skills.ts';
import type { SkillDraft } from '../skills.ts';

export function SkillSettings({ value, disabled, onChange }: {
  value: SkillDraft[]; disabled: boolean; onChange: (value: SkillDraft[]) => void;
}) {
  function update(index: number, change: Partial<SkillDraft>) {
    onChange(value.map((skill, position) => position === index ? { ...skill, ...change } : skill));
  }
  function add() { onChange([...value, emptySkill(value.length + 1)]); }
  return <fieldset className="model-profile skill-settings" disabled={disabled}>
    <legend>受管能力 · Skills</legend>
    <p className="muted">这里仅登记受管 skill 的说明与正文，不授予 shell、MCP、浏览器或插件执行权。启用与信任必须分别明确选择。</p>
    {value.length === 0 && <p className="notice">当前没有已登记的 skill。空列表只有在保存整份配置时才会清除已保存设置。</p>}
    <div className="skill-list">
      {value.map((skill, index) => <fieldset className="skill-entry" key={index}>
        <legend>Skill {index + 1} · {skill.id || '未命名'}</legend>
        <div className="form-grid model-form-grid">
          <label>标识<input value={skill.id} maxLength={256} onChange={(event) => update(index, { id: event.target.value })} required /></label>
          <label>版本<input type="number" min="1" value={skill.version} onChange={(event) => update(index, { version: Number(event.target.value) })} required /></label>
          <label className="wide-field">作用域<input value={skill.scope} maxLength={256} onChange={(event) => update(index, { scope: event.target.value })} required /></label>
          <label className="wide-field">摘要<textarea rows={2} maxLength={2048} value={skill.summary} onChange={(event) => update(index, { summary: event.target.value })} required /></label>
          <label className="wide-field">正文<textarea rows={6} maxLength={16384} value={skill.body} onChange={(event) => update(index, { body: event.target.value })} required /></label>
        </div>
        <p className="muted small">正文 {utf8Bytes(skill.body)} / 16,384 UTF-8 字节；只保存在当前认证页面内存，保存后由 Host 受管存储。</p>
        <div className="button-row model-actions">
          <label className="checkbox-label"><input type="checkbox" checked={skill.enabled} onChange={(event) => update(index, { enabled: event.target.checked })} />启用</label>
          <label className="skill-trust">信任<select value={skill.trust} onChange={(event) => update(index, { trust: event.target.value as ProductSkillTrust })}>
            <option value="untrusted">{skillTrustLabel('untrusted')}（默认）</option>
            <option value="owner_input">{skillTrustLabel('owner_input')}</option>
          </select></label>
          <button type="button" className="secondary-button" onClick={() => onChange(value.filter((_, position) => position !== index))}>移除</button>
        </div>
      </fieldset>)}
    </div>
    <button type="button" className="secondary-button" onClick={add}>添加 skill</button>
  </fieldset>;
}
