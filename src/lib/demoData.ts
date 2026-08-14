import type { UsageBucket, UsageResponse, UsageSession } from "./types";

const sources = [
  { source: "codex", provider: "api.openai.com", model: "gpt-5.6-codex", project: "Metera" },
  { source: "workbuddy", provider: "jrm.ai", model: "claude-sonnet-4.5", project: "Research Radar" },
  { source: "zcode", provider: "free.niuniu.ai", model: "glm-4.7", project: "Claude Desktop" },
  { source: "kimi-code", provider: "api.moonshot.cn", model: "kimi-k2.5", project: "Metera" },
  { source: "dsh", provider: "deepseek", model: "deepseek-v4-pro", project: "DeepseekHarness_WorkSpace" },
];

const tokenScale = [1.1, .72, .58, .34, .9];
export function demoUsage(start: string, end: string): UsageResponse {
  const startAt = new Date(start).getTime();
  const endAt = new Date(end).getTime();
  const now = new Date();
  const buckets: UsageBucket[] = [];
  const sessions: UsageSession[] = [];
  for (let dayOffset = 13; dayOffset >= 0; dayOffset--) {
    for (let hour = 0; hour < 24; hour++) {
      const at = new Date(now); at.setDate(at.getDate() - dayOffset); at.setHours(hour, 0, 0, 0);
      if (at.getTime() < startAt || at.getTime() >= endAt) continue;
      sources.forEach((item, sourceIndex) => {
        const activeHour = ((hour + sourceIndex * 3 + dayOffset) % 7 < 3 && hour >= 8 && hour <= 23)
          || (dayOffset === 0 && hour === now.getHours());
        if (!activeHour) return;
        const pulse = 1 + ((hour * 17 + dayOffset * 11 + sourceIndex * 7) % 9) / 10;
        const input = Math.round(920_000 * tokenScale[sourceIndex] * pulse);
        const cached = Math.round(input * (.52 + sourceIndex * .08));
        const output = Math.round(input * (.08 + sourceIndex * .015));
        const reasoning = Math.round(output * .22);
        buckets.push({ ...item, hostname: sourceIndex % 2 ? "desktop" : "terminal", bucketStart: at.toISOString(), inputTokens: input, cachedInputTokens: cached, outputTokens: output, reasoningOutputTokens: reasoning, totalTokens: input + cached + output + reasoning });
      });
    }
    sources.forEach((item, sourceIndex) => {
      const first = new Date(now); first.setDate(first.getDate() - dayOffset); first.setHours(9 + sourceIndex * 3, 12, 0, 0);
      if (first.getTime() < startAt || first.getTime() >= endAt) return;
      const duration = 1600 + ((dayOffset * 733 + sourceIndex * 977) % 9800);
      const prompts = Array(24).fill(0); prompts[first.getHours()] = 4 + sourceIndex; prompts[Math.min(23, first.getHours() + 1)] = 2 + dayOffset % 5;
      sessions.push({ source: item.source, project: item.project, hostname: sourceIndex % 2 ? "desktop" : "terminal", sessionHash: `demo-${dayOffset}-${sourceIndex}`, firstMessageAt: first.toISOString(), lastMessageAt: new Date(first.getTime() + duration * 1000).toISOString(), durationSeconds: duration, activeSeconds: Math.round(duration * (.38 + sourceIndex * .09)), messageCount: 18 + dayOffset * 2 + sourceIndex * 7, userMessageCount: 6 + dayOffset + sourceIndex * 2, userPromptHours: prompts });
    });
  }
  return { buckets, sessions, hasAnyData: buckets.length > 0 };
}
