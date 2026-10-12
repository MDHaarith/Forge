import Anthropic from "@anthropic-ai/sdk";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { apiKey, baseURL, findProvider, listModels, PROVIDERS } from "./providers.ts";

type BetaMessage = Anthropic.Beta.BetaMessage;
type BetaMessageParam = Anthropic.Beta.BetaMessageParam;
type BetaTool = Anthropic.Beta.BetaTool;

export interface ModelRequest {
  system: string;
  tools: BetaTool[];
  messages: BetaMessageParam[];
}

export type ModelReply = Pick<BetaMessage, "content" | "stop_reason">;

/** What the agent talks to. Claude in normal use; scripted models in tests and offline mode. */
export interface Model {
  readonly name: string;
  respond(request: ModelRequest): Promise<ModelReply>;
}

// FORGE_MODEL names a Claude model only when Claude is the startup provider.
const claudeAtStart = (process.env.FORGE_PROVIDER ?? "claude") === "claude";
export const CLAUDE_MODEL = (claudeAtStart && process.env.FORGE_MODEL) || "claude-opus-5-5";

export class ClaudeModel implements Model {
  readonly name: string;
  private client = new Anthropic();

  constructor(model = CLAUDE_MODEL) {
    this.name = model;
  }

  async respond({ system, tools, messages }: ModelRequest): Promise<ModelReply> {
    for (let attempt = 1; ; attempt++) {
      try {
        const stream = this.client.beta.messages.stream({
          model: this.name,
          max_tokens: 64000,
          // Effort is the main quality/cost dial; workshop tasks are multi-step, so stay at high.
          output_config: { effort: "high" },
          // If a safety classifier declines, the API retries on a suitable model within the same call.
          betas: ["server-side-fallback-2026-07-01"],
          fallbacks: "default",
          system: [{ type: "text", text: system, cache_control: { type: "ephemeral" } }],
          // Large inputs (whole files) stream as they are written; agent.ts validates them before running.
          tools: tools.map((t) => ({ ...t, eager_input_streaming: true })),
          messages,
        });
        return await stream.finalMessage();
      } catch (error) {
        // Typed API errors (auth, rate limits, bad requests) are real failures. Only an unparseable
        // streamed tool input is worth one more try.
        if (error instanceof Anthropic.APIError || attempt >= 2 || !/json/i.test(String(error))) throw error;
      }
    }
  }
}

let toolIds = 0;
const toolUse = (name: string, input: Record<string, unknown>) =>
  ({ type: "tool_use", id: `toolu_offline_${++toolIds}`, name, input }) as unknown as BetaMessage["content"][number];
const text = (t: string) => ({ type: "text", text: t, citations: null }) as BetaMessage["content"][number];

/**
 * Lets Forge run end to end without an API key: it recognises a request for a 3D part,
 * builds a demo enclosure, commits it, and explains how to switch to Claude.
 */
export class OfflineModel implements Model {
  readonly name = "offline demo";

  async respond({ messages }: ModelRequest): Promise<ModelReply> {
    const last = messages[messages.length - 1];
    const blocks = typeof last.content === "string" ? [] : last.content;
    const result = blocks.find((b) => b.type === "tool_result");
    if (result && result.type === "tool_result") {
      const from = this.toolName(messages, result.tool_use_id);
      if (from === "cad_model" && !result.is_error) {
        return { stop_reason: "tool_use", content: [toolUse("git_commit", { message: "Add demo enclosure" })] };
      }
      return {
        stop_reason: "end_turn",
        content: [text("Done. The enclosure is in models/: open demo-enclosure.stl to see it in 3D, or the .scad file to edit it. This is the offline demo; set ANTHROPIC_API_KEY and restart Forge to work with Claude.")],
      };
    }
    const prompt = typeof last.content === "string" ? last.content : blocks.map((b) => (b.type === "text" ? b.text : "")).join(" ");
    if (/enclosure|box|case|part|3d|model|bracket/i.test(prompt)) {
      return {
        stop_reason: "tool_use",
        content: [
          text("Building a 60 × 40 mm enclosure base with four M3 standoffs."),
          toolUse("cad_model", {
            name: "demo-enclosure",
            parts: [
              { shape: "box", size: [60, 40, 2], at: [0, 0, 0] },
              { shape: "box", size: [60, 2, 20], at: [0, 0, 0] },
              { shape: "box", size: [60, 2, 20], at: [0, 38, 0] },
              { shape: "box", size: [2, 40, 20], at: [0, 0, 0] },
              { shape: "box", size: [2, 40, 20], at: [58, 0, 0] },
              ...[[6, 6], [54, 6], [6, 34], [54, 34]].map(([x, y]) => ({ shape: "cylinder", radius: 3, height: 8, at: [x, y, 2] })),
            ],
          }),
        ],
      };
    }
    return {
      stop_reason: "end_turn",
      content: [text("Forge is running in offline demo mode, so I can only build a sample part. Ask me to \"design an enclosure\" to see the 3D pipeline, or set ANTHROPIC_API_KEY and restart Forge to work with Claude on code, 3D, PCB and chip designs.")],
    };
  }

  private toolName(messages: BetaMessageParam[], id: string): string | undefined {
    for (const m of messages) {
      if (typeof m.content === "string") continue;
      for (const b of m.content) if (b.type === "tool_use" && b.id === id) return b.name;
    }
  }
}

type ChatMessage = Record<string, unknown>;

/** Anthropic-shaped history to OpenAI chat messages. Thinking and other Claude-only blocks are dropped. */
export function toChatMessages(system: string, messages: BetaMessageParam[]): ChatMessage[] {
  const out: ChatMessage[] = [{ role: "system", content: system }];
  for (const m of messages) {
    if (typeof m.content === "string") {
      out.push({ role: m.role, content: m.content });
    } else if (m.role === "assistant") {
      const content = m.content.flatMap((b) => (b.type === "text" ? [b.text] : [])).join("\n");
      const calls = m.content.flatMap((b) =>
        b.type === "tool_use" ? [{ id: b.id, type: "function", function: { name: b.name, arguments: JSON.stringify(b.input) } }] : [],
      );
      out.push({ role: "assistant", content: content || null, ...(calls.length ? { tool_calls: calls } : {}) });
    } else {
      for (const b of m.content) {
        if (b.type === "text") out.push({ role: "user", content: b.text });
        if (b.type === "tool_result") {
          const content = typeof b.content === "string" ? b.content : (b.content ?? []).map((c) => (c.type === "text" ? c.text : "")).join("");
          out.push({ role: "tool", tool_call_id: b.tool_use_id, content });
        }
      }
    }
  }
  return out;
}

interface ChatReply {
  choices?: { finish_reason?: string; message?: { content?: string | null; tool_calls?: { id?: string; function: { name: string; arguments?: string } }[] } }[];
}

/** DeepSeek, Qwen, GLM, OpenCode, OpenRouter, OpenAI: anything with an OpenAI-compatible /chat/completions. */
export class OpenAICompatModel implements Model {
  readonly name: string;
  private baseURL: string;
  private apiKey: string;
  private fetch: typeof fetch;

  constructor(model: string, baseURL: string, apiKey: string, fetchImpl: typeof fetch = fetch) {
    this.name = model;
    this.baseURL = baseURL;
    this.apiKey = apiKey;
    this.fetch = fetchImpl;
  }

  // shortcut: not streamed, so long replies show nothing until done; switch to SSE if that hurts.
  // shortcut: DeepSeek thinking mode's reasoning_content is not echoed back; use its non-thinking model.
  async respond({ system, tools, messages }: ModelRequest): Promise<ModelReply> {
    const res = await this.fetch(`${this.baseURL}/chat/completions`, {
      method: "POST",
      headers: { authorization: `Bearer ${this.apiKey}`, "content-type": "application/json" },
      body: JSON.stringify({
        model: this.name,
        messages: toChatMessages(system, messages),
        tools: tools.map((t) => ({ type: "function", function: { name: t.name, description: t.description, parameters: t.input_schema } })),
      }),
    });
    if (!res.ok) throw new Error(`${this.name}: HTTP ${res.status} ${(await res.text()).slice(0, 500)}`);
    const choice = ((await res.json()) as ChatReply).choices?.[0];
    if (!choice) throw new Error(`${this.name}: the reply had no choices`);

    const content: BetaMessage["content"] = [];
    if (choice.message?.content) content.push(text(choice.message.content));
    for (const call of choice.message?.tool_calls ?? []) {
      let input: unknown;
      try {
        input = JSON.parse(call.function.arguments || "{}");
      } catch {
        input = { _unparsed: call.function.arguments }; // validate() in agent.ts reports it back to the model
      }
      // Claude only accepts these id characters, and the history may later go to Claude.
      const id = call.id?.replace(/[^A-Za-z0-9_-]/g, "_") || `toolu_compat_${++toolIds}`;
      content.push({ type: "tool_use", id, name: call.function.name, input } as unknown as BetaMessage["content"][number]);
    }
    const finish = choice.finish_reason;
    const calls = content.some((b) => b.type === "tool_use");
    return {
      content,
      stop_reason: finish === "length" ? "max_tokens" : finish === "content_filter" ? "refusal" : calls ? "tool_use" : "end_turn",
    };
  }
}

/** One model object shared by every project, so the in-app picker can swap what it talks to. */
export class SwitchableModel implements Model {
  provider: string;
  private current: Model;

  constructor(provider: string, current: Model) {
    this.provider = provider;
    this.current = current;
  }

  get name(): string {
    return this.current.name;
  }

  respond(request: ModelRequest): Promise<ModelReply> {
    return this.current.respond(request);
  }

  set(provider: string, model: Model): void {
    this.provider = provider;
    this.current = model;
  }
}

function hasClaudeCredentials(): boolean {
  // The SDK also reads a profile saved by `ant auth login`, so an unset key is not proof of no credentials.
  return Boolean(
    process.env.ANTHROPIC_API_KEY ||
      process.env.ANTHROPIC_AUTH_TOKEN ||
      process.env.ANTHROPIC_PROFILE ||
      fs.existsSync(path.join(os.homedir(), ".config", "anthropic")),
  );
}

/** Provider ids with credentials configured, in picker order; the offline demo is always there. */
export function availableProviders(): string[] {
  return [...(hasClaudeCredentials() ? ["claude"] : []), ...PROVIDERS.filter((p) => apiKey(p)).map((p) => p.id), "offline"];
}

export const providerLabel = (id: string) => (id === "claude" ? "Claude" : id === "offline" ? "Offline demo" : (findProvider(id)?.label ?? id));

export async function modelChoices(id: string): Promise<string[]> {
  if (id === "claude") return [CLAUDE_MODEL];
  if (id === "offline") return ["offline demo"];
  const p = findProvider(id);
  return p ? listModels(p) : [];
}

/** Callers check the id against availableProviders() first. */
export function makeModel(id: string, model?: string): Model {
  if (id === "claude") return new ClaudeModel(model || CLAUDE_MODEL);
  const p = findProvider(id);
  if (!p) return new OfflineModel(); // "offline"
  return new OpenAICompatModel(model || p.defaultModel, baseURL(p), apiKey(p));
}

/** FORGE_PROVIDER when it is configured, else Claude when it has credentials, else the offline demo. */
export function defaultModel(): SwitchableModel {
  const available = availableProviders();
  const wanted = process.env.FORGE_PROVIDER;
  const id = process.env.FORGE_OFFLINE === "1" ? "offline" : wanted && available.includes(wanted) ? wanted
    : available.includes("claude") ? "claude"
    : "offline";
  return new SwitchableModel(id, makeModel(id, claudeAtStart ? undefined : process.env.FORGE_MODEL));
}
