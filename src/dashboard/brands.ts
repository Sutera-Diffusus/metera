// §17.2 品牌色终稿 —— 前端图表统一从这里取色，禁止散落硬编码。
export const SOURCE_COLORS: Record<string, string> = {
  codex: "#e8eaf0",
  "claude-code": "#d97757",
  "kimi-code": "#7a5af8",
  workbuddy: "#32e6b9",
  zcode: "#3b5bff",
  zhipu: "#3b5bff",
  reasonix: "#5eead4",
  dsh: "#4d6bfe",
};

export const BRAND_FALLBACK = "#8e94a8";

export const sourceColor = (source: string) =>
  SOURCE_COLORS[source.toLowerCase()] ?? BRAND_FALLBACK;

// SVG fill/stroke 用：返回 CSS 变量引用，随深浅主题自适应（浅色下 codex 银白 → 石墨）。
// 注意只能用于 style/内联 CSS，不能用作 SVG 表现属性值（属性不支持 var()）。
const SOURCE_VAR_KEYS: Record<string, string> = {
  codex: "codex",
  "claude-code": "claude",
  "kimi-code": "kimi",
  workbuddy: "workbuddy",
  zcode: "zhipu",
  zhipu: "zhipu",
  glm: "zhipu",
  "z.ai": "zhipu",
  reasonix: "reasonix",
  dsh: "dsh",
};

export const sourceFill = (source: string) => {
  const key = SOURCE_VAR_KEYS[source.toLowerCase()];
  return key ? `var(--brand-${key})` : "var(--brand-unknown)";
};

// 单位色（沿用浮窗 §10）
export const UNIT_COLORS = { tokens: "#ff453a", cost: "#30d158" } as const;

// Reasonix 签名渐变（官方 logo.svg：cyan → violet → fuchsia）
export const REASONIX_GRADIENT = ["#5eead4", "#93c5fd", "#c4b5fd"] as const;

// 模型名 → 品牌 key（用于模型 icon 与配色归属；匹配规则对齐 pricing 的规范化思路）
const MODEL_BRAND_RULES: Array<[RegExp, string]> = [
  [/deepseek/i, "deepseek"],
  [/claude|opus|sonnet|haiku|fable|mythos/i, "claude"],
  [/kimi|k2\.|k3|moonshot/i, "kimi"],
  [/gpt|o[134]-|codex/i, "openai"],
  [/glm|zhipu/i, "zhipu"],
  [/z\.ai|\bzai\b/i, "zai"],
  [/qwen/i, "qwen"],
  [/hy3|hunyuan/i, "hunyuan"],
];

export const modelBrand = (model: string): string | null => {
  const bare = model.split("/").pop() ?? model;
  for (const [pattern, brand] of MODEL_BRAND_RULES) {
    if (pattern.test(model) || pattern.test(bare)) return brand;
  }
  return null;
};

// 模型品牌节点配色（桑基右列用；openai 取中性银灰，深浅主题都可读）
export const MODEL_BRAND_COLORS: Record<string, string> = {
  openai: "#9aa3b2",
  claude: "#d97757",
  kimi: "#7a5af8",
  moonshot: "#7a5af8",
  deepseek: "#4d6bfe",
  zai: "#3b5bff",
  zhipu: "#3b5bff",
  qwen: "#615ced",
  hunyuan: "#00b2a9",
  codebuddy: "#ff8c42",
};

export const modelBrandColor = (model: string) => {
  const brand = modelBrand(model);
  return brand ? MODEL_BRAND_COLORS[brand] ?? BRAND_FALLBACK : BRAND_FALLBACK;
};

// 额度账号 provider → 图标品牌 key
export const providerBrand = (provider: string): string =>
  ({ codex: "codex", kimi: "kimi-code", "kimi-code": "kimi-code", claude: "claude-code", "claude-code": "claude-code", workbuddy: "workbuddy", zcode: "zhipu", zai: "zhipu", zhipu: "zhipu", reasonix: "reasonix", dsh: "dsh", deepseek: "dsh" }[provider.toLowerCase()] ?? provider.toLowerCase());
