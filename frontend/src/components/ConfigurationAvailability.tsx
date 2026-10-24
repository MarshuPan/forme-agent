import type { ProductConfigAvailability, ProductSettingFeature } from '../types.ts';

const labels: Record<ProductSettingFeature, string> = {
  models: '模型', skills: '技能', mcp: 'MCP 工具', plugins: '插件', packages: '能力包',
  channels: '消息渠道', proxy: '网络代理', network: 'Web 入口',
};

export function ConfigurationAvailability({ value }: { value?: ProductConfigAvailability }) {
  if (!value) return null;
  return <section><h3>此设备的设置范围</h3><ul className="service-list">
    {value.features.map((item) => <li key={item.feature}><span>{labels[item.feature]}</span>
      <span>{item.state === 'configurable' ? '可配置'
        : item.reason === 'network_paths_missing' ? '需要先升级安装配置' : '尚未接通受管设置'}</span></li>)}
  </ul><p className="muted">可配置表示已接通设置入口；实际使用还需相应授权和运行检查。</p></section>;
}
