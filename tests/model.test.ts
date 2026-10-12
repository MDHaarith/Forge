import assert from "node:assert/strict";
import { test } from "node:test";
import { OpenAICompatModel } from "../server/model.ts";

/** A fetch that records the request body and answers with `reply`. */
function fakeFetch(reply: unknown, status = 200) {
  const sent: Record<string, unknown>[] = [];
  const impl = (async (url: string, init: RequestInit) => {
    assert.equal(url, "https://example.test/v1/chat/completions");
    assert.equal((init.headers as Record<string, string>).authorization, "Bearer key");
    sent.push(JSON.parse(String(init.body)));
    return new Response(JSON.stringify(reply), { status });
  }) as typeof fetch;
  return { impl, sent };
}

const tools = [{ name: "cad_model", description: "Build a part", input_schema: { type: "object" as const, properties: {} } }];

test("history and tools are sent in OpenAI chat format", async () => {
  const { impl, sent } = fakeFetch({ choices: [{ finish_reason: "stop", message: { content: "Done." } }] });
  const model = new OpenAICompatModel("m1", "https://example.test/v1", "key", impl);
  const reply = await model.respond({
    system: "sys",
    tools,
    messages: [
      { role: "user", content: "make a box" },
      {
        role: "assistant",
        content: [
          { type: "thinking", thinking: "hmm", signature: "s" },
          { type: "text", text: "Building." },
          { type: "tool_use", id: "t1", name: "cad_model", input: { name: "box" } },
        ],
      },
      { role: "user", content: [{ type: "tool_result", tool_use_id: "t1", content: "ok" }] },
    ],
  });
  assert.deepEqual(sent[0].messages, [
    { role: "system", content: "sys" },
    { role: "user", content: "make a box" },
    { role: "assistant", content: "Building.", tool_calls: [{ id: "t1", type: "function", function: { name: "cad_model", arguments: '{"name":"box"}' } }] },
    { role: "tool", tool_call_id: "t1", content: "ok" },
  ]);
  assert.deepEqual(sent[0].tools, [{ type: "function", function: { name: "cad_model", description: "Build a part", parameters: tools[0].input_schema } }]);
  assert.equal(sent[0].model, "m1");
  assert.equal(reply.stop_reason, "end_turn");
  assert.deepEqual(reply.content.map((b) => b.type), ["text"]);
});

test("tool calls, cut-off replies and bad arguments map back", async () => {
  const run = async (choice: unknown) =>
    new OpenAICompatModel("m", "https://example.test/v1", "key", fakeFetch({ choices: [choice] }).impl).respond({ system: "", tools, messages: [{ role: "user", content: "x" }] });

  const called = await run({ finish_reason: "tool_calls", message: { content: null, tool_calls: [{ id: "functions.cad:0", function: { name: "cad_model", arguments: '{"a":1}' } }] } });
  assert.equal(called.stop_reason, "tool_use");
  assert.deepEqual(called.content, [{ type: "tool_use", id: "functions_cad_0", name: "cad_model", input: { a: 1 } }]);

  const bad = await run({ finish_reason: "tool_calls", message: { tool_calls: [{ id: "c", function: { name: "cad_model", arguments: "{oops" } }] } });
  assert.deepEqual((bad.content[0] as { input: unknown }).input, { _unparsed: "{oops" });

  assert.equal((await run({ finish_reason: "length", message: { content: "par" } })).stop_reason, "max_tokens");
  assert.equal((await run({ finish_reason: "content_filter", message: {} })).stop_reason, "refusal");
});

test("HTTP errors carry the status and body", async () => {
  const model = new OpenAICompatModel("m", "https://example.test/v1", "key", fakeFetch({ error: "bad key" }, 401).impl);
  await assert.rejects(model.respond({ system: "", tools, messages: [{ role: "user", content: "x" }] }), /HTTP 401.*bad key/);
});
