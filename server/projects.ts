import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { Agent, type AgentEvent } from "./agent.ts";
import type { Model } from "./model.ts";
import { availableTools } from "./tools/registry.ts";
import { initRepo } from "./tools/vcs.ts";

const NAME = /^[a-z0-9][a-z0-9-_]{0,63}$/;

interface Session {
  agent: Agent;
  events: AgentEvent[];
}

/**
 * Projects are folders under the Forge home (default ~/Forge), each its own git repository.
 * Conversations are saved beside them in .state/ so they survive restarts without being committed.
 */
export class Projects {
  private sessions = new Map<string, Session>();

  readonly home: string;
  private model: Model;

  constructor(home: string, model: Model) {
    this.home = home;
    this.model = model;
  }

  static defaultHome(): string {
    return process.env.FORGE_HOME ?? path.join(os.homedir(), "Forge");
  }

  async list(): Promise<string[]> {
    await fs.mkdir(this.home, { recursive: true });
    const entries = await fs.readdir(this.home, { withFileTypes: true });
    return entries.filter((e) => e.isDirectory() && NAME.test(e.name)).map((e) => e.name).sort();
  }

  dir(name: string): string {
    if (!NAME.test(name)) throw new Error("project names use lowercase letters, digits, - and _");
    return path.join(this.home, name);
  }

  async exists(name: string): Promise<boolean> {
    return NAME.test(name) && (await fs.stat(this.dir(name)).catch(() => null))?.isDirectory() === true;
  }

  async create(name: string): Promise<void> {
    const dir = this.dir(name);
    if (await this.exists(name)) throw new Error(`project ${name} already exists`);
    await fs.mkdir(dir, { recursive: true });
    await fs.writeFile(path.join(dir, "README.md"), `# ${name}\n\nA Forge project. Designs live here as text: code, models/, pcb/, chip/, fab/.\n`);
    await initRepo(dir);
  }

  /** True while any project's agent is working. */
  running(): boolean {
    return [...this.sessions.values()].some((s) => s.agent.running);
  }

  private statePath(name: string): string {
    return path.join(this.home, ".state", `${name}.json`);
  }

  async session(name: string): Promise<Session> {
    let s = this.sessions.get(name);
    if (!s) {
      const agent = new Agent(this.model, name, this.dir(name), availableTools);
      s = { agent, events: [] };
      const saved = await fs.readFile(this.statePath(name), "utf8").then(JSON.parse).catch(() => null);
      if (saved) {
        agent.messages.push(...saved.messages);
        s.events.push(...saved.events);
      }
      this.sessions.set(name, s);
    }
    return s;
  }

  async chat(name: string, text: string, emit: (e: AgentEvent) => void): Promise<void> {
    const s = await this.session(name);
    await s.agent.send(text, (e) => {
      s.events.push(e);
      emit(e);
    });
    await fs.mkdir(path.dirname(this.statePath(name)), { recursive: true });
    await fs.writeFile(this.statePath(name), JSON.stringify({ messages: s.agent.messages, events: s.events }));
  }
}
