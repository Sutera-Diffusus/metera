import claudeIcon from "../assets/brands/claude.svg";
import codebuddyIcon from "../assets/brands/codebuddy.svg";
import deepseekIcon from "../assets/brands/deepseek.svg";
import dshIcon from "../assets/brands/dsh.svg";
import hunyuanIcon from "../assets/brands/hunyuan.svg";
import kimiIcon from "../assets/brands/kimi.svg";
import moonshotIcon from "../assets/brands/moonshot.svg";
import openaiIcon from "../assets/brands/openai.svg";
import qwenIcon from "../assets/brands/qwen.svg";
import reasonixAppIcon from "../assets/brands/reasonix-icon.svg";
import workbuddyColorIcon from "../assets/brands/workbuddy-color.svg";
import zaiIcon from "../assets/brands/zai.svg";
import zhipuIcon from "../assets/brands/zhipu.svg";

// tone：dark-art = 图标本身是深色稿（暗色主题需反白）；light-art = 图标本身是白色稿（浅色主题需反黑）；color = 彩色原稿不处理。
interface IconDef { src: string; tone: "dark-art" | "light-art" | "color" }

const ICONS: Record<string, IconDef> = {
  // 七源
  codex: { src: openaiIcon, tone: "dark-art" },
  "claude-code": { src: claudeIcon, tone: "dark-art" },
  "kimi-code": { src: kimiIcon, tone: "dark-art" },
  workbuddy: { src: workbuddyColorIcon, tone: "color" },
  zcode: { src: zhipuIcon, tone: "dark-art" },
  reasonix: { src: reasonixAppIcon, tone: "color" },
  dsh: { src: dshIcon, tone: "dark-art" },
  // 模型品牌（modelBrand() 的输出 key）
  deepseek: { src: deepseekIcon, tone: "dark-art" },
  claude: { src: claudeIcon, tone: "dark-art" },
  openai: { src: openaiIcon, tone: "dark-art" },
  kimi: { src: kimiIcon, tone: "dark-art" },
  moonshot: { src: moonshotIcon, tone: "dark-art" },
  zai: { src: zaiIcon, tone: "dark-art" },
  zhipu: { src: zhipuIcon, tone: "dark-art" },
  qwen: { src: qwenIcon, tone: "dark-art" },
  hunyuan: { src: hunyuanIcon, tone: "dark-art" },
  codebuddy: { src: codebuddyIcon, tone: "dark-art" },
};

export function BrandIcon({ brand, size = 16, className = "" }: { brand: string; size?: number; className?: string }) {
  const key = brand.toLowerCase() === "zcode" ? "zhipu" : brand.toLowerCase();
  const def = ICONS[key];
  if (!def) return null;
  return <img className={`brand-icon tone-${def.tone} ${className}`} src={def.src} width={size} height={size} alt="" aria-hidden="true" draggable={false}/>;
}
