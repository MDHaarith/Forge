import type { AssemblyPart, BoardBoxes, FitIssue } from "./design.ts";
import { renderMarkdown } from "./markdown.ts";

type AgentEvent =
  | { type: "user"; text: string }
  | { type: "text"; text: string }
  | { type: "tool_call"; id: string; name: string; input: unknown }
  | { type: "tool_result"; id: string; name: string; ok: boolean; text: string; files: string[] }
  | { type: "error"; message: string }
  | { type: "done" };

interface ToolInfo {
  name: string;
  domain: string;
  description: string;
  available: boolean;
  requires?: string;
  install?: string;
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const el = (tag: string, cls?: string, text?: string) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};

const DOMAINS: Record<string, string> = {
  workspace: "Workspace",
  vcs: "Version control",
  code: "Code",
  "3d": "3D / mechanical",
  pcb: "PCB",
  chip: "Chip",
};

const BINARY = new Set(["step", "stp", "fcstd", "iges", "igs", "brep", "blend", "obj", "zip", "vvp", "pdf", "dxf", "kicad_pcb"]);
const IMAGES: Record<string, string> = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", webp: "image/webp", svg: "image/svg+xml" };
// After a tool runs, show its most telling output: a drawing, then a render, then a model.
const PREVIEW_ORDER = ["svg", "png", "glb", "stl"];
/** Icon colour group for the file tree. */
const KINDS: Record<string, string> = {
  stl: "model", glb: "model", step: "model", stp: "model", fcstd: "model", blend: "model", obj: "model", scad: "model",
  svg: "drawing", pdf: "drawing", dxf: "drawing", png: "drawing", jpg: "drawing", jpeg: "drawing", webp: "drawing",
  kicad_pcb: "board",
};

let project: string | null = localStorage.getItem("forge.project");
let closeViewer: (() => void) | null = null;
let files: string[] = [];
/** File extension (with dot) -> desktop program that opens it, for installed programs only. */
let openers: Record<string, string> = {};
const toolCards = new Map<string, HTMLElement>();
let mode: "chat" | "design" = localStorage.getItem("forge.mode") === "design" ? "design" : "chat";
let closeDesign: (() => void) | null = null;
/** Bumped by each loadDesign, so a slower earlier load stops instead of drawing over a newer one. */
let designRun = 0;
let showSchematic = false;

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, init);
  if (!res.ok) throw new Error((await res.json().catch(() => null))?.error ?? res.statusText);
  return res.json();
}

const post = (body: unknown): RequestInit => ({
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(body),
});

async function loadStatus() {
  const status = await api<{ model: string; offline: boolean; tools: ToolInfo[]; open: Record<string, string> }>("/status");
  openers = status.open ?? {};
  $("caps-summary").textContent = `Tools ${status.tools.filter((t) => t.available).length}/${status.tools.length}`;
  await loadModels();

  const host = $("capabilities");
  host.replaceChildren();
  const shown = new Set<string>();
  for (const [domain, label] of Object.entries(DOMAINS)) {
    const tools = status.tools.filter((t) => t.domain === domain);
    const group = el("div", "cap-group");
    group.append(el("div", "cap-title", label));
    for (const t of tools) {
      const row = el("div", `cap ${t.available ? "on" : "off"}`);
      row.title = t.available ? t.description : `Needs ${t.requires}${t.install ? `: ${t.install}` : ""}`;
      row.append(el("span", "dot"), el("span", "cap-name", t.name));
      if (!t.available) row.append(el("span", "cap-need", `needs ${t.requires}`));
      group.append(row);
      // One install line per missing program, not one per tool that needs it.
      if (!t.available && t.install && !shown.has(t.install)) {
        shown.add(t.install);
        group.append(el("code", "cap-install", t.install));
      }
    }
    host.append(group);
  }
}

interface ProviderInfo {
  id: string;
  label: string;
  models: string[];
}
let providers: ProviderInfo[] = [];

async function loadModels() {
  const state = await api<{ provider: string; model: string; effort: string | null; efforts: string[]; providers: ProviderInfo[] }>("/models");
  providers = state.providers;
  const pick = $<HTMLSelectElement>("provider-pick");
  pick.replaceChildren(...providers.map((p) => new Option(p.label, p.id, false, p.id === state.provider)));
  const offline = state.provider === "offline";
  pick.classList.toggle("warn", offline);
  pick.title = offline ? "Add an API key to ~/.config/forge/env and restart Forge to use a real model" : "Model provider";
  const models = providers.find((p) => p.id === state.provider)?.models ?? [];
  const names = models.includes(state.model) ? models : [state.model, ...models];
  $("model-name").replaceChildren(
    ...names.map((m) => new Option(m, m, false, m === state.model)),
    new Option("Custom model…", ""),
  );
  // Only Claude Code takes a thinking level today.
  const effort = $<HTMLSelectElement>("effort-pick");
  effort.hidden = state.provider !== "claude";
  effort.replaceChildren(
    new Option("Thinking: default", "", false, !state.effort),
    ...state.efforts.map((e) => new Option(`Thinking: ${e}`, e, false, e === state.effort)),
  );
}

async function switchModel(model?: string, effort?: string) {
  const provider = $<HTMLSelectElement>("provider-pick").value;
  try {
    await api("/model", post({ provider, model: model ?? $<HTMLSelectElement>("model-name").value, effort }));
  } catch (e) {
    alert((e as Error).message);
  }
  await loadModels();
}

$("provider-pick").addEventListener("change", () => {
  // A new provider starts on its first listed model; the old model name means nothing there.
  const provider = $<HTMLSelectElement>("provider-pick").value;
  void switchModel(providers.find((p) => p.id === provider)?.models[0]);
});
$("model-name").addEventListener("change", () => {
  const picked = $<HTMLSelectElement>("model-name").value;
  const model = picked || prompt("Model name")?.trim();
  if (model) void switchModel(model);
  else void loadModels();
});
$("effort-pick").addEventListener("change", () => void switchModel(undefined, $<HTMLSelectElement>("effort-pick").value));

async function loadProjects() {
  const names = await api<string[]>("/projects");
  const list = $("projects");
  list.replaceChildren();
  for (const name of names) {
    const item = el("li", name === project ? "active" : "", name);
    item.onclick = () => openProject(name);
    list.append(item);
  }
  if (project && !names.includes(project)) project = null;
}

async function loadFiles() {
  const list = $("files");
  list.replaceChildren();
  if (!project) return;
  files = await api<string[]>(`/projects/${project}/files`);
  $("status-project").textContent = `${project} · ${files.length} files`;
  // An outliner-style tree: each folder once, before its first file.
  const folders = new Set<string>();
  const indent = (depth: number) => `${10 + depth * 14}px`;
  for (const file of files) {
    const parts = file.split("/");
    for (let d = 1; d < parts.length; d++) {
      const dir = parts.slice(0, d).join("/");
      if (folders.has(dir)) continue;
      folders.add(dir);
      const row = el("li", "folder", parts[d - 1]);
      row.style.paddingLeft = indent(d - 1);
      list.append(row);
    }
    const item = el("li", "", parts.at(-1));
    const ext = file.toLowerCase().split(".").pop() ?? "";
    item.dataset.kind = KINDS[ext] ?? (file.endsWith(".board.json") ? "board" : "code");
    item.dataset.file = file;
    item.title = file;
    item.style.paddingLeft = indent(parts.length - 1);
    item.classList.toggle("active", file === $("preview-title").textContent);
    item.onclick = () => openFile(file);
    list.append(item);
  }
  fillDesignPicker();
}

async function openProject(name: string) {
  project = name;
  localStorage.setItem("forge.project", name);
  $("project-title").textContent = name;
  $<HTMLTextAreaElement>("prompt").disabled = false;
  $<HTMLButtonElement>("send").disabled = false;
  await Promise.all([loadProjects(), loadFiles()]);
  const { events } = await api<{ events: AgentEvent[] }>(`/projects/${name}/history`);
  $("log").replaceChildren();
  toolCards.clear();
  events.forEach(render);
  if (!events.length) $("log").append(el("div", "empty", "Describe what you want to build in this project."));
  if (mode === "design") await loadDesign();
}

/** A file picked by the user shows in the viewport, so leave the Design workspace for it. */
function openFile(file: string) {
  if (mode === "design") setMode("chat");
  void preview(file);
}

function showOpenButton(file: string) {
  const button = $<HTMLButtonElement>("open-app");
  const ext = "." + (file.toLowerCase().split(".").pop() ?? "");
  const app = openers[ext];
  button.hidden = !app;
  if (!app) return;
  button.textContent = `Open in ${app}`;
  button.onclick = async () => {
    button.disabled = true;
    try {
      await api(`/projects/${project}/open`, post({ path: file }));
      button.textContent = `Opened in ${app}`;
    } catch (error) {
      alert(error instanceof Error ? error.message : String(error));
    } finally {
      setTimeout(() => {
        button.disabled = false;
        button.textContent = `Open in ${app}`;
      }, 2500);
    }
  };
}

function showImage(host: HTMLElement, url: string, alt: string, drawing: boolean) {
  const img = el("img", `render${drawing ? " drawing" : ""}`) as HTMLImageElement;
  img.src = url;
  img.alt = alt;
  const expand = () => {
    const big = el("img", drawing ? "drawing" : "") as HTMLImageElement;
    big.src = url;
    big.alt = alt;
    const box = $("lightbox");
    box.replaceChildren(big);
    box.hidden = false;
  };
  img.onclick = expand;
  const button = $<HTMLButtonElement>("expand");
  button.hidden = false;
  button.onclick = expand;
  host.replaceChildren(img);
}

async function preview(file: string) {
  if (!project) return;
  closeViewer?.();
  closeViewer = null;
  $("preview-title").textContent = file;
  for (const row of $("files").querySelectorAll<HTMLElement>("li[data-file]")) row.classList.toggle("active", row.dataset.file === file);
  $("expand").hidden = true;
  showOpenButton(file);
  const host = $("preview");
  const ext = file.toLowerCase().split(".").pop() ?? "";
  // An exact CAD file has no browser preview; show the mesh FreeCAD exported next to it.
  let shown = file;
  if (["step", "stp", "fcstd"].includes(ext)) {
    const mesh = file.replace(/\.[^.]+$/, ".stl");
    if (files.includes(mesh)) shown = mesh;
  }
  const res = await fetch(`/api/projects/${project}/file?path=${encodeURIComponent(shown)}`);
  if (!res.ok) return host.replaceChildren(el("div", "empty", "Could not open this file."));
  const kind = shown.toLowerCase().split(".").pop() ?? "";
  if (kind === "stl" || kind === "glb") {
    // three.js is large, so the viewer loads the first time a model is opened.
    const { showModel } = await import("./viewer.ts");
    host.replaceChildren();
    closeViewer = await showModel(host, await res.arrayBuffer(), kind);
    if (shown !== file) host.prepend(el("div", "note", `Showing the mesh ${shown}. The exact model is ${file}.`));
  } else if (IMAGES[kind]) {
    const url = URL.createObjectURL(new Blob([await res.arrayBuffer()], { type: IMAGES[kind] }));
    showImage(host, url, file, kind === "svg");
    closeViewer = () => URL.revokeObjectURL(url);
  } else if (BINARY.has(kind)) {
    const why: Record<string, string> = {
      pdf: "the printable drawing; the .svg next to it shows the same sheet here",
      dxf: "a 2D outline for laser cutting or CNC; open it in your CAM program",
    };
    host.replaceChildren(el("div", "empty", `${file} is ${why[kind] ?? "a binary file, for its own program"}.`));
  } else if (kind === "md" || kind === "markdown") {
    const doc = el("article", "md doc");
    doc.innerHTML = renderMarkdown(await res.text());
    host.replaceChildren(doc);
  } else {
    const text = await res.text();
    host.replaceChildren(el("pre", "code", text.length > 100_000 ? text.slice(0, 100_000) + "\n…" : text));
  }
}

function render(e: AgentEvent) {
  const log = $("log");
  log.querySelector(".empty")?.remove();
  switch (e.type) {
    case "user":
      log.append(el("div", "msg user", e.text));
      break;
    case "text": {
      const msg = el("div", "msg assistant md");
      msg.innerHTML = renderMarkdown(e.text);
      log.append(msg);
      break;
    }
    case "tool_call": {
      const card = el("details", "tool running");
      const summary = el("summary");
      const input = (e.input ?? {}) as Record<string, unknown>;
      const hint = [input.path, input.source, input.name, input.message].find((v) => typeof v === "string") as string | undefined;
      summary.append(el("span", "dot"), el("span", "tool-name", e.name.replaceAll("_", " ")));
      if (hint) summary.append(el("span", "tool-hint", hint));
      card.append(summary, el("pre", "code", JSON.stringify(e.input, null, 2)));
      toolCards.set(e.id, card);
      log.append(card);
      break;
    }
    case "tool_result": {
      const card = toolCards.get(e.id);
      if (!card) break;
      card.classList.remove("running");
      card.classList.add(e.ok ? "ok" : "failed");
      card.append(el("pre", "code result", e.text));
      for (const file of e.files.filter((f) => !f.endsWith("/"))) {
        const link = el("button", "file-link", file);
        link.onclick = () => openFile(file);
        card.querySelector("summary")!.append(link);
      }
      break;
    }
    case "error":
      log.append(el("div", "msg error", e.message));
      break;
    case "done":
      // A stopped turn leaves calls without results.
      for (const card of log.querySelectorAll(".tool.running")) card.classList.replace("running", "failed");
      break;
  }
  log.scrollTop = log.scrollHeight;
}

async function send(message: string) {
  if (!project) return;
  const button = $<HTMLButtonElement>("send");
  button.textContent = "Stop";
  button.classList.add("stop");
  button.type = "button";
  button.onclick = () => {
    button.disabled = true;
    void api(`/projects/${project}/stop`, post({})).catch(() => {});
  };
  let toPreview: string | undefined;
  let toDesign: string | undefined;
  try {
    const res = await fetch(`/api/projects/${project}/chat`, post({ message }));
    if (!res.ok || !res.body) throw new Error((await res.json().catch(() => null))?.error ?? res.statusText);
    const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = "";
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += value;
      const lines = buffer.split("\n");
      buffer = lines.pop()!;
      for (const line of lines.filter(Boolean)) {
        const event = JSON.parse(line) as AgentEvent;
        render(event);
        if (event.type === "tool_result" && event.files.length) {
          // Prefer a render, then a model with materials, then a plain mesh.
          for (const ext of PREVIEW_ORDER) {
            const hit = event.files.find((f) => f.toLowerCase().endsWith(`.${ext}`));
            if (hit) {
              toPreview = hit;
              break;
            }
          }
          // A new board or assembly opens in the Design view, like a Fusion 360 electronics design.
          toDesign = event.files.find((f) => /^design\/[^/]+\.json$/.test(f)) ?? event.files.find((f) => /^pcb\/.+\.board\.json$/.test(f)) ?? toDesign;
          await loadFiles();
        }
      }
    }
  } catch (error) {
    render({ type: "error", message: error instanceof Error ? error.message : String(error) });
  } finally {
    button.disabled = false;
    button.textContent = "Send";
    button.classList.remove("stop");
    button.type = "submit";
    button.onclick = null;
  }
  if (toDesign) {
    fillDesignPicker(toDesign);
    if (mode === "design") await loadDesign();
    else setMode("design");
  } else if (toPreview) await preview(toPreview);
}

$("composer").addEventListener("submit", (ev) => {
  ev.preventDefault();
  const input = $<HTMLTextAreaElement>("prompt");
  const message = input.value.trim();
  if (!message) return;
  input.value = "";
  void send(message);
});

$("prompt").addEventListener("keydown", (ev) => {
  if (ev.key === "Enter" && !ev.shiftKey && !ev.isComposing) {
    ev.preventDefault();
    if ($<HTMLButtonElement>("send").type === "submit") $<HTMLFormElement>("composer").requestSubmit();
  }
});

$("new-project").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const input = $<HTMLInputElement>("new-project-name");
  try {
    await api("/projects", post({ name: input.value.trim() }));
    const name = input.value.trim();
    input.value = "";
    await openProject(name);
  } catch (error) {
    alert(error instanceof Error ? error.message : String(error));
  }
});

$("lightbox").addEventListener("click", () => {
  $("lightbox").hidden = true;
});
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") $("lightbox").hidden = true;
});
// The status bar's tools list is a <details>; close it on any click outside, like a menu.
document.addEventListener("click", (ev) => {
  const caps = document.querySelector(".caps");
  if (caps && !caps.contains(ev.target as Node)) caps.removeAttribute("open");
});

// ---- Design view: the PCB beside the 3D assembly ----------------------------------------------

interface DesignFile {
  parts: { file: string; position: [number, number, number]; rotation: [number, number, number]; color?: string; opacity?: number }[];
  clearance: number;
  fit: FitIssue[];
}

/** Assemblies (design/*.json) first, then boards (pcb/*.board.json) that can be shown on their own. */
function designChoices(): { value: string; label: string }[] {
  const designs = files.filter((f) => /^design\/[^/]+\.json$/.test(f)).map((f) => ({ value: f, label: `Assembly · ${f.slice(7, -5)}` }));
  const boards = files.filter((f) => /^pcb\/.+\.board\.json$/.test(f)).map((f) => ({ value: f, label: `Board · ${f.slice(4, -11)}` }));
  return [...designs, ...boards];
}

function fillDesignPicker(prefer?: string) {
  const pick = $<HTMLSelectElement>("design-pick");
  const current = prefer ?? pick.value ?? localStorage.getItem(`forge.design.${project}`);
  pick.replaceChildren();
  const choices = designChoices();
  for (const c of choices) {
    const option = el("option", "", c.label) as HTMLOptionElement;
    option.value = c.value;
    pick.append(option);
  }
  if (!choices.length) pick.append(el("option", "", "No board or assembly yet"));
  const saved = current || localStorage.getItem(`forge.design.${project}`);
  if (saved && choices.some((c) => c.value === saved)) pick.value = saved;
  pick.disabled = !choices.length;
}

async function fetchFile(file: string): Promise<ArrayBuffer> {
  const res = await fetch(`/api/projects/${project}/file?path=${encodeURIComponent(file)}`);
  if (!res.ok) throw new Error(`could not load ${file}`);
  return res.arrayBuffer();
}

const fetchJson = async <T>(file: string): Promise<T> => JSON.parse(new TextDecoder().decode(await fetchFile(file)));

async function loadDesign() {
  const run = ++designRun;
  closeDesign?.();
  closeDesign = null;
  const view2d = $("design-2d");
  const view3d = $("design-3d");
  const chips = $("design-parts");
  const badge = $("fit-status");
  chips.replaceChildren();
  badge.textContent = "";
  badge.className = "badge";
  const choice = $<HTMLSelectElement>("design-pick").value;
  if (!project || !designChoices().some((c) => c.value === choice)) {
    view2d.replaceChildren(el("div", "empty", "No board yet. Ask Forge for a circuit board (it writes pcb/<name>.tsx and runs pcb_build); it shows here."));
    view3d.replaceChildren(el("div", "empty", "Then ask for an enclosure and to assemble the board in it (design_assemble): the 3D assembly and its fit check show here."));
    return;
  }
  localStorage.setItem(`forge.design.${project}`, choice);
  view2d.replaceChildren(el("div", "empty", "Loading…"));
  view3d.replaceChildren(el("div", "empty", "Loading…"));

  try {
    const design: DesignFile = choice.startsWith("design/")
      ? await fetchJson<DesignFile>(choice)
      : { parts: [{ file: choice.replace(/\.board\.json$/, ".glb"), position: [0, 0, 0], rotation: [0, 0, 0] }], clearance: 0, fit: [] };
    const parts: AssemblyPart[] = [];
    for (const p of design.parts) {
      const boardData = p.file.endsWith(".glb") ? p.file.replace(/\.glb$/, ".board.json") : undefined;
      const board = boardData && files.includes(boardData) ? await fetchJson<BoardBoxes>(boardData) : undefined;
      parts.push({ ...p, data: await fetchFile(p.file), board });
    }
    const main = parts.find((p) => p.board);
    const { showAssembly, showBoardSvg, partKey } = await import("./design.ts");
    const base = main?.file.replace(/\.glb$/, "");
    const svgFile = base && `${base}-${showSchematic ? "schematic" : "pcb"}.svg`;
    const svgText = svgFile && files.includes(svgFile) ? new TextDecoder().decode(await fetchFile(svgFile)) : undefined;
    if (run !== designRun) return;

    // Fit status.
    const collisions = design.fit.filter((f) => f.status === "collision").length;
    const tight = design.fit.length - collisions;
    const checked = choice.startsWith("design/") && parts.some((p) => p.board) && parts.some((p) => !p.board);
    if (checked) {
      badge.textContent = collisions ? `${collisions} collision${collisions > 1 ? "s" : ""}` : tight ? `Fits · ${tight} tight` : `Fits · ≥ ${design.clearance} mm clear`;
      badge.classList.add(collisions ? "bad" : tight ? "warn" : "ok");
      badge.title = design.fit.map((f) => `${f.item} ${f.status === "collision" ? "cuts into" : "is tight to"} ${f.part}`).join("\n");
    }

    // 2D: the main board's PCB or schematic.
    $("pane-2d-title").textContent = showSchematic ? "Schematic" : "PCB";
    const issuesByName = new Map<string, string>();
    for (const f of design.fit) if (f.board === main?.file && f.item !== "board" && issuesByName.get(f.item) !== "collision") issuesByName.set(f.item, f.status);

    let select: (key: string | null) => void = () => {};
    let board2d: { select(name: string | null): void; dispose(): void } | undefined;
    if (svgText) {
      board2d = showBoardSvg(view2d, svgText, showSchematic ? undefined : main?.board, issuesByName, (name) => select(name && main ? partKey(main.file, name) : null));
    } else {
      view2d.replaceChildren(el("div", "empty", showSchematic ? "No schematic for this board." : "No PCB drawing for this board."));
    }

    view3d.replaceChildren();
    const assembly = await showAssembly(view3d, parts, design.fit, (key) => select(key));
    if (run !== designRun) {
      assembly.dispose();
      board2d?.dispose();
      return;
    }
    assembly.setSeeThrough($<HTMLInputElement>("xray").checked);

    // Part chips: every part on the main board, problems first.
    const chipFor = new Map<string, HTMLElement>();
    const order = (name: string) => (issuesByName.get(name) === "collision" ? 0 : issuesByName.get(name) === "tight" ? 1 : 2);
    if (main?.board) {
      if (design.fit.some((f) => f.board === main.file && f.item === "board")) {
        const s = design.fit.find((f) => f.board === main.file && f.item === "board" && f.status === "collision") ? "collision" : "tight";
        chips.append(el("span", `part-chip ${s}`, `board ${s}`));
      }
      for (const c of [...main.board.components].sort((a, b) => order(a.name) - order(b.name) || a.name.localeCompare(b.name, undefined, { numeric: true }))) {
        const chip = el("button", `part-chip ${issuesByName.get(c.name) ?? ""}`, c.name);
        const height = Math.round((c.max[2] - main.board.size[2] / 2) * 10) / 10;
        chip.title = `${c.name}: ${issuesByName.get(c.name) ?? "clear"}, ${height} mm tall above the board`;
        chip.onclick = () => select(partKey(main.file, c.name));
        chipFor.set(partKey(main.file, c.name), chip);
        chips.append(chip);
      }
    }
    let selected: string | null = null;
    select = (key) => {
      selected = key === selected ? null : key;
      assembly.select(selected);
      board2d?.select(selected && main && selected.startsWith(`${main.file}|`) ? selected.slice(main.file.length + 1) : null);
      for (const [k, chip] of chipFor) chip.classList.toggle("selected", k === selected);
    };
    $("xray").onchange = () => assembly.setSeeThrough($<HTMLInputElement>("xray").checked);
    closeDesign = () => {
      assembly.dispose();
      board2d?.dispose();
    };
  } catch (error) {
    if (run === designRun) view3d.replaceChildren(el("div", "empty", `Could not show the design: ${error instanceof Error ? error.message : String(error)}`));
  }
}

function setMode(next: "chat" | "design") {
  mode = next;
  localStorage.setItem("forge.mode", next);
  const design = next === "design";
  document.querySelector(".layout")!.classList.toggle("design-mode", design);
  $("design").hidden = !design;
  for (const [id, on] of [["mode-chat", !design], ["mode-design", design]] as const) {
    $(id).classList.toggle("active", on);
    $(id).setAttribute("aria-pressed", String(on));
  }
  if (design) void loadDesign();
  else {
    closeDesign?.();
    closeDesign = null;
  }
}

$("mode-chat").addEventListener("click", () => setMode("chat"));
$("mode-design").addEventListener("click", () => setMode("design"));
$("design-pick").addEventListener("change", () => void loadDesign());
for (const [id, schematic] of [["show-pcb", false], ["show-schematic", true]] as const) {
  $(id).addEventListener("click", () => {
    showSchematic = schematic;
    $("show-pcb").classList.toggle("active", !schematic);
    $("show-schematic").classList.toggle("active", schematic);
    void loadDesign();
  });
}

await loadStatus();
await loadProjects();
setMode(mode);
if (project) await openProject(project);
