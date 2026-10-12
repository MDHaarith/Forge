import type Anthropic from "@anthropic-ai/sdk";
import type { Model } from "./model.ts";
import { ToolError, type ForgeTool } from "./tools/types.ts";
import { validate } from "./tools/validate.ts";

type BetaMessageParam = Anthropic.Beta.BetaMessageParam;
type BetaToolResultBlockParam = Anthropic.Beta.BetaToolResultBlockParam;

export type AgentEvent =
  | { type: "user"; text: string }
  | { type: "text"; text: string }
  | { type: "tool_call"; id: string; name: string; input: unknown }
  | { type: "tool_result"; id: string; name: string; ok: boolean; text: string; files: string[] }
  | { type: "error"; message: string }
  | { type: "done" };

const MAX_STEPS = 40;

export function systemPrompt(projectName: string, tools: ForgeTool[]): string {
  const domains = [...new Set(tools.map((t) => t.domain))].join(", ");
  return `You are Forge, an engineering assistant that runs a personal workshop: software, 3D/mechanical parts, printed circuit boards and chips. You act through tools on the user's project "${projectName}", a git repository on their machine.

Available domains right now: ${domains}. Domains with no tools here need software the user has not installed; when a request needs one, say which program to install instead of pretending.

How to work:
- Keep every design as text in the project (code, OpenSCAD, Verilog, KiCad files, notes) so it can be diffed, reviewed and rebuilt.
- Use the check tools (simulation, DRC, synthesis, builds) on your own output before calling something finished, and report what passed or failed.
- Commit with git_commit after each meaningful step, with a one-line message.
- Use millimetres for mechanical work. State key dimensions and assumptions in your reply.
- Pick the 3D tool by purpose: freecad_build for engineering parts that need exact dimensions, holes, fits or a STEP file for manufacturing; blender_build for visual and organic models, scenes, materials and renders; cad_model only for quick block mock-ups when neither is available. Keep the .py script in the project (cad/ for FreeCAD, art/ for Blender) and re-run it after edits.
- For FreeCAD parts: use the forge helpers for standard holes, standoffs and enclosures instead of guessing sizes; check the result with freecad_inspect (holes, mass, interference between mating parts, print overhangs) and fix what does not meet the request; make a freecad_drawing when the part will be manufactured or the user wants a drawing.
- For circuit boards: write the board as tscircuit code in pcb/<name>.tsx and build it with pcb_build (Forge's own PCB engine; no KiCad needed). Fix every error it reports and rebuild until it says every connection is routed. Use KiCad tools only when the user works in KiCad.
- For radios and antennas: get trace widths for 50 ohm feeds and impedance-controlled pairs from rf_calc and use them in the board code; pour ground under RF traces but keep it out from under the antenna; put a pi matching footprint between the radio and the antenna. When rf_antenna_sim is available, simulate the antenna after pcb_build, fix its length if the resonance is off, and size the matching network from the simulated impedance with rf_calc match. Check range with rf_calc link. Prefer a pre-certified radio module unless the user wants a custom RF design.
- For electronics in an enclosure, design both together like in Fusion 360: build the board, then size the enclosure in FreeCAD from the board's size, hole positions and tallest part, then call design_assemble with the board's .glb and the enclosure parts. Fix every collision (move parts, the board or the enclosure feature) and re-run it; the Design view shows the PCB beside the 3D assembly.
- Manufacturing means producing files ready to order (STL for printing, Gerbers for a PCB fab, GDS for a shuttle). You cannot place orders; tell the user what to upload where.
- Be brief in chat: what you made, where it is, what to check.`;
}

/**
 * One project's conversation with the model. The message history is append-only:
 * assistant turns are stored exactly as returned, which keeps thinking blocks valid
 * and the prompt cache warm.
 */
export class Agent {
  readonly messages: BetaMessageParam[] = [];
  private busy = false;

  private model: Model;
  private projectName: string;
  private projectDir: string;
  private tools: () => ForgeTool[];

  constructor(model: Model, projectName: string, projectDir: string, tools: () => ForgeTool[]) {
    this.model = model;
    this.projectName = projectName;
    this.projectDir = projectDir;
    this.tools = tools;
  }

  get running(): boolean {
    return this.busy;
  }

  async send(userText: string, emit: (e: AgentEvent) => void): Promise<void> {
    if (this.busy) throw new Error("Forge is still working on the previous request");
    this.busy = true;
    try {
      emit({ type: "user", text: userText });
      this.messages.push({ role: "user", content: userText });
      await this.loop(emit);
    } catch (error) {
      emit({ type: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      this.busy = false;
      emit({ type: "done" });
    }
  }

  private async loop(emit: (e: AgentEvent) => void): Promise<void> {
    const tools = this.tools();
    const byName = new Map(tools.map((t) => [t.name, t]));
    const system = systemPrompt(this.projectName, tools);
    const definitions = tools.map(({ name, description, input_schema }) => ({ name, description, input_schema }));

    for (let step = 0; step < MAX_STEPS; step++) {
      const reply = await this.model.respond({ system, tools: definitions, messages: this.messages });
      this.messages.push({ role: "assistant", content: reply.content });

      for (const block of reply.content) if (block.type === "text" && block.text.trim()) emit({ type: "text", text: block.text });

      const calls = reply.content.filter((b) => b.type === "tool_use");
      // A cut-off or declined turn can end inside a tool input: answer those calls without running them.
      const halted =
        reply.stop_reason === "max_tokens" ? "the reply hit its length limit before this call was complete"
        : reply.stop_reason === "refusal" ? "the request was declined"
        : null;

      if (calls.length === 0) {
        if (reply.stop_reason === "pause_turn") continue;
        if (halted) emit({ type: "error", message: `Stopped: ${halted}.` });
        return;
      }

      const results: BetaToolResultBlockParam[] = [];
      for (const call of calls) {
        emit({ type: "tool_call", id: call.id, name: call.name, input: call.input });
        const outcome = halted
          ? { ok: false, text: `Not run: ${halted}.`, files: [] }
          : await this.execute(byName.get(call.name), call.name, call.input);
        emit({ type: "tool_result", id: call.id, name: call.name, ...outcome });
        results.push({ type: "tool_result", tool_use_id: call.id, content: outcome.text, ...(outcome.ok ? {} : { is_error: true }) });
      }
      // All results go back in one message so parallel calls stay parallel.
      this.messages.push({ role: "user", content: results });
      if (halted) {
        emit({ type: "error", message: `Stopped: ${halted}.` });
        return;
      }
    }
    emit({ type: "error", message: `Stopped after ${MAX_STEPS} steps; send a message to continue.` });
  }

  private async execute(tool: ForgeTool | undefined, name: string, input: unknown) {
    if (!tool) return { ok: false, text: `Unknown or unavailable tool "${name}".`, files: [] };
    const problems = validate(input, tool.input_schema as Parameters<typeof validate>[1]);
    if (problems.length) {
      return { ok: false, text: JSON.stringify({ INVALID_INPUT: problems, received: input }), files: [] };
    }
    try {
      const result = await tool.run(input as Record<string, unknown>, { projectDir: this.projectDir });
      return { ok: true, text: result.text, files: result.files ?? [] };
    } catch (error) {
      const message = error instanceof ToolError ? error.message : `${name} failed: ${error instanceof Error ? error.message : error}`;
      return { ok: false, text: message, files: [] };
    }
  }
}
