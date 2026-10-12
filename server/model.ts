import Anthropic from "@anthropic-ai/sdk";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

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

export const CLAUDE_MODEL = process.env.FORGE_MODEL ?? "claude-opus-5-5";

export class ClaudeModel implements Model {
  readonly name = CLAUDE_MODEL;
  private client = new Anthropic();

  async respond({ system, tools, messages }: ModelRequest): Promise<ModelReply> {
    for (let attempt = 1; ; attempt++) {
      try {
        const stream = this.client.beta.messages.stream({
          model: CLAUDE_MODEL,
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

/** Claude when credentials are configured, otherwise the offline demo. */
export function defaultModel(): Model {
  // The SDK also reads a profile saved by `ant auth login`, so an unset key is not proof of no credentials.
  const hasCredentials =
    process.env.ANTHROPIC_API_KEY ||
    process.env.ANTHROPIC_AUTH_TOKEN ||
    process.env.ANTHROPIC_PROFILE ||
    fs.existsSync(path.join(os.homedir(), ".config", "anthropic"));
  return hasCredentials && process.env.FORGE_OFFLINE !== "1" ? new ClaudeModel() : new OfflineModel();
}
