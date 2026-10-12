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

let project: string | null = localStorage.getItem("forge.project");
let closeViewer: (() => void) | null = null;
let files: string[] = [];
/** File extension (with dot) -> desktop program that opens it, for installed programs only. */
let openers: Record<string, string> = {};
const toolCards = new Map<string, HTMLElement>();

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
  const badge = $("model-badge");
  badge.textContent = status.offline ? "Offline demo" : status.model;
  badge.classList.toggle("warn", status.offline);
  badge.title = status.offline ? "Set ANTHROPIC_API_KEY and restart Forge to use Claude" : "Model in use";

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
  for (const file of files) {
    const item = el("li", "", file);
    item.onclick = () => preview(file);
    list.append(item);
  }
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
    case "text":
      log.append(el("div", "msg assistant", e.text));
      break;
    case "tool_call": {
      const card = el("details", "tool running");
      const summary = el("summary");
      summary.append(el("span", "dot"), el("span", "tool-name", e.name));
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
        link.onclick = () => preview(file);
        card.querySelector("summary")!.append(link);
      }
      break;
    }
    case "error":
      log.append(el("div", "msg error", e.message));
      break;
  }
  log.scrollTop = log.scrollHeight;
}

async function send(message: string) {
  if (!project) return;
  const button = $<HTMLButtonElement>("send");
  button.disabled = true;
  button.textContent = "Working…";
  let toPreview: string | undefined;
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
          await loadFiles();
        }
      }
    }
  } catch (error) {
    render({ type: "error", message: error instanceof Error ? error.message : String(error) });
  } finally {
    button.disabled = false;
    button.textContent = "Send";
  }
  if (toPreview) await preview(toPreview);
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
  if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) $<HTMLFormElement>("composer").requestSubmit();
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

await loadStatus();
await loadProjects();
if (project) await openProject(project);
