/**
 * OpenAI-compatible providers. Subscription "coding plans" (GLM, Qwen, OpenCode) are an API key on a
 * plan-specific endpoint: point FORGE_<ID>_BASE_URL at it. Keys live in ~/.config/forge/env.
 */
export interface Provider {
  id: string;
  label: string;
  baseURL: string;
  keyEnv: string;
  /** Used until the provider's model list loads; any model it lists can be picked instead. */
  defaultModel: string;
}

export const PROVIDERS: Provider[] = [
  { id: "openai", label: "OpenAI", baseURL: "https://api.openai.com/v1", keyEnv: "OPENAI_API_KEY", defaultModel: "gpt-5" },
  { id: "openrouter", label: "OpenRouter", baseURL: "https://openrouter.ai/api/v1", keyEnv: "OPENROUTER_API_KEY", defaultModel: "openai/gpt-5" },
  { id: "deepseek", label: "DeepSeek", baseURL: "https://api.deepseek.com/v1", keyEnv: "DEEPSEEK_API_KEY", defaultModel: "deepseek-chat" },
  { id: "qwen", label: "Qwen", baseURL: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", keyEnv: "DASHSCOPE_API_KEY", defaultModel: "qwen3-coder-plus" },
  { id: "glm", label: "GLM (Z.ai)", baseURL: "https://api.z.ai/api/coding/paas/v4", keyEnv: "ZAI_API_KEY", defaultModel: "glm-4.6" },
  { id: "opencode", label: "OpenCode Zen", baseURL: "https://opencode.ai/zen/v1", keyEnv: "OPENCODE_API_KEY", defaultModel: "qwen3-coder" },
];

export const findProvider = (id: string) => PROVIDERS.find((p) => p.id === id);
export const baseURL = (p: Provider) => (process.env[`FORGE_${p.id.toUpperCase()}_BASE_URL`] ?? p.baseURL).replace(/\/+$/, "");
export const apiKey = (p: Provider) => process.env[p.keyEnv] ?? "";

const lists = new Map<string, Promise<string[]>>();

/** The provider's model ids from GET /models, cached; failures are not cached, so a later call retries. */
export function listModels(p: Provider): Promise<string[]> {
  let list = lists.get(p.id);
  if (!list) {
    list = fetch(`${baseURL(p)}/models`, { headers: { authorization: `Bearer ${apiKey(p)}` }, signal: AbortSignal.timeout(5000) })
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(r.statusText))))
      .then((body: { data?: { id: string }[] }) => (body.data ?? []).map((m) => m.id).sort())
      .catch(() => {
        lists.delete(p.id);
        return [];
      });
    lists.set(p.id, list);
  }
  return list.then((ids) => (ids.length ? ids : [p.defaultModel]));
}
