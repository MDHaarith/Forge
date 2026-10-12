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

let project: string | null = localStorage.getItem("forge.project");
let closeViewer: (() => void) | null = null;
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
  const status = await api<{ model: string; offline: boolean; tools: ToolInfo[] }>("/status");
  const badge = $("model-badge");
  badge.textContent = status.offline ? "Offline demo" : status.model;
  badge.classList.toggle("warn", status.offline);
  badge.title = status.offline ? "Set ANTHROPIC_API_KEY and restart Forge to use Claude" : "Model in use";

  const host = $("capabilities");
  host.replaceChildren();
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
  for (const file of await api<string[]>(`/projects/${project}/files`)) {
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

async function preview(file: string) {
  if (!project) return;
  closeViewer?.();
  closeViewer = null;
  $("preview-title").textContent = file;
  const host = $("preview");
  const res = await fetch(`/api/projects/${project}/file?path=${encodeURIComponent(file)}`);
  if (!res.ok) return host.replaceChildren(el("div", "empty", "Could not open this file."));
  if (file.toLowerCase().endsWith(".stl")) {
    // three.js is large, so the viewer loads the first time a model is opened.
    const { showStl } = await import("./viewer.ts");
    closeViewer = showStl(host, await res.arrayBuffer());
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
  let lastStl: string | undefined;
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
          lastStl = event.files.find((f) => f.endsWith(".stl")) ?? lastStl;
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
  if (lastStl) await preview(lastStl);
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

await loadStatus();
await loadProjects();
if (project) await openProject(project);
