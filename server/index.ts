import "./env.ts";
import fs from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defaultModel, type Model } from "./model.ts";
import { Projects } from "./projects.ts";
import { describeTools } from "./tools/registry.ts";
import { projectPath } from "./tools/paths.ts";
import { listFiles } from "./tools/workspace.ts";

const DIST = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "dist");

const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
};

class HttpError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

function json(res: http.ServerResponse, status: number, body: unknown) {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
}

async function readJson(req: http.IncomingMessage): Promise<Record<string, unknown>> {
  // Requiring JSON forces a CORS preflight, so other websites cannot post to Forge.
  if (!req.headers["content-type"]?.startsWith("application/json")) throw new HttpError(415, "expected application/json");
  let body = "";
  for await (const chunk of req) {
    body += chunk;
    if (body.length > 1_000_000) throw new HttpError(413, "request too large");
  }
  return JSON.parse(body || "{}");
}

export function createServer(projects: Projects, model: Model): http.Server {
  return http.createServer(async (req, res) => {
    try {
      // Only answer to localhost names, which blocks DNS-rebinding attacks from web pages.
      const host = (req.headers.host ?? "").replace(/:\d+$/, "");
      if (!["127.0.0.1", "localhost", "[::1]"].includes(host)) throw new HttpError(403, "forbidden host");

      const url = new URL(req.url ?? "/", "http://localhost");
      const parts = url.pathname.split("/").filter(Boolean).map(decodeURIComponent);

      if (parts[0] !== "api") return await serveStatic(url.pathname, res);

      if (req.method === "GET" && parts[1] === "status" && parts.length === 2) {
        return json(res, 200, { model: model.name, offline: model.name === "offline demo", home: projects.home, tools: describeTools() });
      }
      if (parts[1] === "projects" && parts.length === 2) {
        if (req.method === "GET") return json(res, 200, await projects.list());
        if (req.method === "POST") {
          const { name } = await readJson(req);
          await projects.create(String(name ?? "")).catch((e) => {
            throw new HttpError(400, e.message);
          });
          return json(res, 201, { name });
        }
      }
      if (parts[1] === "projects" && parts.length === 4) {
        const name = parts[2];
        if (!(await projects.exists(name))) throw new HttpError(404, "no such project");
        const dir = projects.dir(name);
        const action = parts[3];

        if (req.method === "GET" && action === "files") return json(res, 200, await listFiles(dir));
        if (req.method === "GET" && action === "file") {
          let full: string;
          try {
            full = projectPath(dir, url.searchParams.get("path") ?? "");
          } catch (e) {
            throw new HttpError(400, (e as Error).message);
          }
          const data = await fs.readFile(full).catch(() => {
            throw new HttpError(404, "no such file");
          });
          res.writeHead(200, { "content-type": "application/octet-stream" });
          return res.end(data);
        }
        if (req.method === "GET" && action === "history") {
          const s = await projects.session(name);
          return json(res, 200, { events: s.events, running: s.agent.running });
        }
        if (req.method === "POST" && action === "chat") {
          const { message } = await readJson(req);
          if (typeof message !== "string" || !message.trim()) throw new HttpError(400, "message is required");
          if ((await projects.session(name)).agent.running) throw new HttpError(409, "Forge is still working on this project");
          // Events stream back as newline-delimited JSON while the agent works.
          res.writeHead(200, { "content-type": "application/x-ndjson", "cache-control": "no-cache" });
          await projects.chat(name, message.trim(), (e) => res.write(JSON.stringify(e) + "\n"));
          return res.end();
        }
      }
      throw new HttpError(404, "not found");
    } catch (error) {
      const status = error instanceof HttpError ? error.status : 500;
      if (res.headersSent) return res.end();
      json(res, status, { error: error instanceof Error ? error.message : String(error) });
    }
  });
}

async function serveStatic(pathname: string, res: http.ServerResponse) {
  const rel = pathname === "/" ? "index.html" : pathname.slice(1);
  const full = path.resolve(DIST, rel);
  if (!full.startsWith(DIST + path.sep)) throw new HttpError(404, "not found");
  const data = await fs.readFile(full).catch(() => null);
  if (!data) {
    if (pathname === "/") throw new HttpError(503, "the interface is not built yet: run `npm start`, or `npm run dev` for development");
    throw new HttpError(404, "not found");
  }
  res.writeHead(200, { "content-type": TYPES[path.extname(full)] ?? "application/octet-stream" });
  res.end(data);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const model = defaultModel();
  const projects = new Projects(Projects.defaultHome(), model);
  const port = Number(process.env.PORT ?? 4317);
  // The desktop app holds our stdin open; when it closes (the app quit or crashed), stop too.
  if (process.env.FORGE_EXIT_WITH_STDIN === "1") process.stdin.on("end", () => process.exit(0)).resume();
  createServer(projects, model).listen(port, "127.0.0.1", () => {
    console.log(`Forge is running at http://127.0.0.1:${port}`);
    console.log(`Model: ${model.name}${model.name === "offline demo" ? " (set ANTHROPIC_API_KEY to use Claude)" : ""}`);
    console.log(`Projects: ${projects.home}`);
  });
}
