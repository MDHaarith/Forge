import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { Agent, type AgentEvent } from "../server/agent.ts";
import type { Model, ModelReply, ModelRequest } from "../server/model.ts";
import { allTools } from "../server/tools/registry.ts";

type Block = ModelReply["content"][number];
const text = (t: string) => ({ type: "text", text: t, citations: null }) as Block;
const call = (id: string, name: string, input: unknown) => ({ type: "tool_use", id, name, input }) as unknown as Block;

/** Replies from a fixed script and records every request it receives. */
class ScriptedModel implements Model {
  readonly name = "scripted";
  requests: ModelRequest[] = [];
  private replies: ModelReply[];

  constructor(replies: ModelReply[]) {
    this.replies = replies;
  }

  async respond(request: ModelRequest): Promise<ModelReply> {
    this.requests.push(structuredClone(request));
    const reply = this.replies.shift();
    if (!reply) throw new Error("script ran out");
    return reply;
  }
}

async function setup(replies: ModelReply[]) {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "forge-agent-"));
  const model = new ScriptedModel(replies);
  const agent = new Agent(model, "test", dir, () => allTools.filter((t) => t.domain === "workspace" || t.domain === "3d"));
  const events: AgentEvent[] = [];
  return { dir, model, agent, events, emit: (e: AgentEvent) => events.push(e) };
}

test("runs tools, feeds results back, and keeps history append-only", async () => {
  const { dir, model, agent, events, emit } = await setup([
    { stop_reason: "tool_use", content: [text("Writing it."), call("t1", "write_file", { path: "src/hello.txt", contents: "hi" })] },
    { stop_reason: "end_turn", content: [text("Done.")] },
  ]);
  await agent.send("make a file", emit);

  assert.equal(await fs.readFile(path.join(dir, "src/hello.txt"), "utf8"), "hi");
  assert.deepEqual(events.map((e) => e.type), ["user", "text", "tool_call", "tool_result", "text", "done"]);
  const result = events.find((e) => e.type === "tool_result");
  assert.ok(result?.type === "tool_result" && result.ok && result.files[0] === "src/hello.txt");

  // The second request starts with the first request's messages unchanged.
  const [first, second] = model.requests;
  assert.deepEqual(second.messages.slice(0, first.messages.length), first.messages);
  assert.deepEqual(agent.messages.map((m) => m.role), ["user", "assistant", "user", "assistant"]);
  assert.ok(first.tools.some((t) => t.name === "cad_model"));
  assert.ok(!first.tools.some((t) => t.name === "git_commit"));
});

test("invalid input, unknown tools and tool errors come back as error results", async () => {
  const { agent, events, emit, model } = await setup([
    {
      stop_reason: "tool_use",
      content: [
        call("a", "write_file", { path: "x.txt" }),
        call("b", "launch_rocket", {}),
        call("c", "read_file", { path: "../../etc/passwd" }),
      ],
    },
    { stop_reason: "end_turn", content: [text("ok")] },
  ]);
  await agent.send("go", emit);
  const results = events.filter((e) => e.type === "tool_result");
  assert.equal(results.length, 3);
  assert.ok(results.every((r) => r.type === "tool_result" && !r.ok));
  // All three results return together in one user message.
  const last = model.requests[1].messages.at(-1)!;
  assert.ok(Array.isArray(last.content) && last.content.length === 3 && last.content.every((b) => b.type === "tool_result" && b.is_error));
});

test("a reply cut off at max_tokens does not run its tool call", async () => {
  const { dir, agent, events, emit } = await setup([
    { stop_reason: "max_tokens", content: [call("t", "write_file", { path: "half.txt", contents: "trunc" })] },
  ]);
  await agent.send("go", emit);
  await assert.rejects(fs.stat(path.join(dir, "half.txt")));
  assert.ok(events.some((e) => e.type === "error"));
  // The dangling call is still answered, so the next request is valid.
  assert.equal(agent.messages.at(-1)!.role, "user");
});

test("cad_model writes STL, SCAD and JSON", async () => {
  const { dir, agent, emit } = await setup([
    {
      stop_reason: "tool_use",
      content: [call("t", "cad_model", { name: "bracket", parts: [{ shape: "box", size: [10, 10, 2] }, { shape: "cylinder", radius: 2, height: 5, at: [5, 5, 2] }] })],
    },
    { stop_reason: "end_turn", content: [] },
  ]);
  await agent.send("bracket", emit);
  const files = await fs.readdir(path.join(dir, "models"));
  assert.deepEqual(files.sort(), ["bracket.json", "bracket.scad", "bracket.stl"]);
});
