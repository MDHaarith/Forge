import { readFileSync } from "node:fs";

/** Fedora is the primary platform; the others get best-effort install hints. */
export type Distro = "fedora" | "debian" | "macos" | "other";

/** How to install a program on each platform. `flatpak` is a Flathub app ID. */
export interface InstallHint {
  dnf?: string;
  flatpak?: string;
  apt?: string;
  brew?: string;
  url?: string;
}

/** Reads /etc/os-release style text. Fedora relatives (RHEL, CentOS Stream, Nobara, Ultramarine) count as Fedora. */
export function detectDistro(osRelease?: string, platform: NodeJS.Platform = process.platform): Distro {
  if (platform === "darwin") return "macos";
  if (osRelease === undefined) {
    try {
      osRelease = readFileSync("/etc/os-release", "utf8");
    } catch {
      return "other";
    }
  }
  const field = (key: string) => new RegExp(`^${key}="?([^"\\n]*)"?`, "m").exec(osRelease!)?.[1]?.toLowerCase() ?? "";
  const ids = `${field("ID")} ${field("ID_LIKE")}`.split(/\s+/);
  if (ids.some((id) => ["fedora", "rhel", "centos"].includes(id))) return "fedora";
  if (ids.some((id) => ["debian", "ubuntu"].includes(id))) return "debian";
  return "other";
}

let current: Distro | undefined;
export function currentDistro(): Distro {
  return (current ??= detectDistro());
}

/** The one command a user on `distro` should run, falling back to the download page. */
export function installText(hint: InstallHint | undefined, distro: Distro = currentDistro()): string | undefined {
  if (!hint) return undefined;
  const flatpak = hint.flatpak && `flatpak install flathub ${hint.flatpak}`;
  const choice =
    distro === "fedora" ? (hint.dnf ? `sudo dnf install ${hint.dnf}` : flatpak)
    : distro === "debian" ? (hint.apt ? `sudo apt install ${hint.apt}` : flatpak)
    : distro === "macos" ? hint.brew && `brew install ${hint.brew}`
    : flatpak;
  return choice ?? hint.url;
}
