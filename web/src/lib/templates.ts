import { Database, ServerCog } from "lucide-react";
import type { ComponentType, SVGProps } from "react";
import type { PortMap, Resources } from "./types";

export type TemplateIcon = ComponentType<SVGProps<SVGSVGElement>>;

export interface TemplateEnv {
  key: string;
  /** Literal default shown in the input. */
  defaultValue?: string;
  /**
   * If set, a 🎲 button next to the input fills a strong random
   * string. `password` → 24 URL-safe chars.
   */
  generate?: "password";
  required?: boolean;
  description?: string;
  /**
   * If true, the value is locked — the template author set it and
   * the user shouldn't fiddle (e.g. PGDATA for Postgres).
   */
  readonly?: boolean;
}

export interface TemplateVolume {
  mountPath: string;
  defaultSizeGb: number;
}

export interface Template {
  id: string;
  name: string;
  description: string;
  icon: TemplateIcon;
  image: string;
  defaultSlug: string;
  defaultName: string;
  ports: PortMap[];
  envVars: TemplateEnv[];
  resources: Resources;
  volume?: TemplateVolume;
  /** Rendered below the deploy form. Supports plain text. */
  notes?: string;
}

export const TEMPLATES: Template[] = [
  {
    id: "postgres",
    name: "PostgreSQL",
    description:
      "PostgreSQL 18 with persistent storage. Pre-sets PGDATA so a fresh volume initialises cleanly.",
    icon: Database,
    image: "postgres:18-alpine",
    defaultSlug: "postgres",
    defaultName: "Postgres",
    ports: [{ container_port: 5432, host_port: null, protocol: "tcp" }],
    envVars: [
      {
        key: "POSTGRES_PASSWORD",
        generate: "password",
        required: true,
        description:
          "Superuser password. Generate one — it will only show here once.",
      },
      {
        key: "POSTGRES_USER",
        defaultValue: "postgres",
        description: "Superuser name.",
      },
      {
        key: "POSTGRES_DB",
        defaultValue: "app",
        description: "Database created on first boot.",
      },
      {
        key: "PGDATA",
        defaultValue: "/var/lib/postgresql/data/pgdata",
        readonly: true,
        description:
          "Subdirectory under the mount — Postgres refuses to initialise in a mount point with lost+found. Leave as-is.",
      },
    ],
    resources: { cpu_millis: 500, memory_mb: 512, disk_mb: 2048 },
    volume: { mountPath: "/var/lib/postgresql/data", defaultSizeGb: 10 },
    notes:
      "Reachable from other services in this project at postgres.driftbase.internal:5432 on the private network. The credentials above are what you configured just now.",
  },
  {
    id: "redis",
    name: "Redis",
    description:
      "Redis with a persistent data volume. No auth — keep it off public ports.",
    icon: ServerCog,
    image: "redis:8.6.3-alpine",
    defaultSlug: "redis",
    defaultName: "Redis",
    ports: [{ container_port: 6379, host_port: null, protocol: "tcp" }],
    envVars: [],
    resources: { cpu_millis: 250, memory_mb: 128, disk_mb: 1024 },
    volume: { mountPath: "/data", defaultSizeGb: 10 },
    notes:
      "Reachable at redis.driftbase.internal:6379 inside the project private network. Authentication is off by default — only expose on domains if you want public access (and enable auth if you do).",
  },
];

export function findTemplate(id: string): Template | undefined {
  return TEMPLATES.find((t) => t.id === id);
}

/**
 * Cryptographically random URL-safe password. Uses base64url so the
 * result is copy-pasteable into a connection string without escaping.
 */
export function generateRandomPassword(length = 24): string {
  const byteLen = Math.ceil((length * 3) / 4);
  const bytes = new Uint8Array(byteLen);
  crypto.getRandomValues(bytes);
  let s = btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "");
  return s.slice(0, length);
}

export function slugify(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40);
}
