import { LingXiModelCatalog } from "./catalog.ts";
import { decodeCatalog } from "./decoder.ts";
import { ModelCatalogError } from "./errors.ts";

export const DEFAULT_CATALOG_ENDPOINT = "https://models.lingxifox.cn/models.json";

export interface ModelCatalogConfiguration {
  readonly endpoint?: string;
  readonly maxAgeMs?: number;
  readonly requestTimeoutMs?: number;
  readonly userAgent?: string;
  readonly storage?: CatalogStorage;
}

export interface CatalogTransportResponse {
  readonly status: number;
  readonly body: Uint8Array;
  readonly etag?: string | null;
}

export interface CatalogTransport {
  fetchCatalog(
    url: string,
    headers: Record<string, string>,
    timeoutMs: number
  ): Promise<CatalogTransportResponse>;
}

export interface CatalogStorage {
  read(): Promise<{ raw: Uint8Array; etag?: string | null; fetchedAt?: number } | null>;
  write(raw: Uint8Array, etag: string | null, fetchedAt: number): Promise<void>;
  updateMetadata(etag: string | null, fetchedAt: number): Promise<void>;
}

export class FetchCatalogTransport implements CatalogTransport {
  async fetchCatalog(
    url: string,
    headers: Record<string, string>,
    timeoutMs: number
  ): Promise<CatalogTransportResponse> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const resp = await fetch(url, {
        method: "GET",
        headers,
        signal: controller.signal,
      });
      const buf = new Uint8Array(await resp.arrayBuffer());
      return {
        status: resp.status,
        body: buf,
        etag: resp.headers.get("etag"),
      };
    } catch (err) {
      throw new ModelCatalogError("transport", `Network error: ${String(err)}`, { cause: err });
    } finally {
      clearTimeout(timer);
    }
  }
}

export class FileSystemCatalogStorage implements CatalogStorage {
  private readonly directory: string;

  constructor(directory: string) {
    this.directory = directory;
  }

  async read(): Promise<{ raw: Uint8Array; etag?: string | null; fetchedAt?: number } | null> {
    const fs = await import("node:fs/promises");
    const path = await import("node:path");
    try {
      const raw = await fs.readFile(path.join(this.directory, "models.json"));
      let etag: string | null = null;
      let fetchedAt: number | undefined;
      try {
        const metaText = await fs.readFile(
          path.join(this.directory, "models.json.meta.json"),
          "utf8"
        );
        const meta = JSON.parse(metaText) as { etag?: string | null; fetchedAt?: string };
        etag = meta.etag ?? null;
        if (meta.fetchedAt) {
          const parsed = Date.parse(meta.fetchedAt);
          if (!Number.isNaN(parsed)) fetchedAt = parsed;
        }
      } catch {
        // Optional metadata file.
      }
      return { raw: new Uint8Array(raw), etag, fetchedAt };
    } catch {
      return null;
    }
  }

  async write(raw: Uint8Array, etag: string | null, fetchedAt: number): Promise<void> {
    const fs = await import("node:fs/promises");
    const path = await import("node:path");
    await fs.mkdir(this.directory, { recursive: true });
    await writeAtomic(fs, path, path.join(this.directory, "models.json"), raw);
    await this.updateMetadata(etag, fetchedAt);
  }

  async updateMetadata(etag: string | null, fetchedAt: number): Promise<void> {
    const fs = await import("node:fs/promises");
    const path = await import("node:path");
    await fs.mkdir(this.directory, { recursive: true });
    const payload = new TextEncoder().encode(
      JSON.stringify({ etag, fetchedAt: new Date(fetchedAt).toISOString() })
    );
    await writeAtomic(fs, path, path.join(this.directory, "models.json.meta.json"), payload);
  }
}

async function writeAtomic(
  fs: typeof import("node:fs/promises"),
  path: typeof import("node:path"),
  target: string,
  data: Uint8Array
): Promise<void> {
  const tmp = path.join(
    path.dirname(target),
    `.cache-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`
  );
  await fs.writeFile(tmp, data);
  await fs.rename(tmp, target);
}

export interface CachedModelCatalog {
  readonly catalog: LingXiModelCatalog;
  readonly fetchedAt: number;
  readonly etag: string | null;
}

export class ModelCatalogLoader {
  private readonly endpoint: string;
  private readonly maxAgeMs: number;
  private readonly requestTimeoutMs: number;
  private readonly userAgent: string;
  private readonly storage: CatalogStorage | undefined;
  private readonly transport: CatalogTransport;
  private memory: CachedModelCatalog | null = null;

  constructor(config: ModelCatalogConfiguration = {}, transport?: CatalogTransport) {
    this.endpoint = config.endpoint ?? DEFAULT_CATALOG_ENDPOINT;
    this.maxAgeMs = config.maxAgeMs ?? 1_800_000;
    this.requestTimeoutMs = config.requestTimeoutMs ?? 30_000;
    this.userAgent = config.userAgent ?? "LingXiModelSDK-TS/2.0";
    this.storage = config.storage;
    this.transport = transport ?? new FetchCatalogTransport();
  }

  async cached(): Promise<CachedModelCatalog | null> {
    if (this.memory !== null) return this.memory;
    if (!this.storage) return null;
    const stored = await this.storage.read();
    if (!stored) return null;
    try {
      const catalog = decodeCatalog(stored.raw);
      const fetchedAt =
        stored.fetchedAt ?? (catalog.revision.generatedAt?.getTime() ?? 0);
      this.memory = { catalog, fetchedAt, etag: stored.etag ?? null };
      return this.memory;
    } catch {
      return null;
    }
  }

  async load(forceRefresh = false): Promise<LingXiModelCatalog> {
    const cached = await this.cached();
    const now = Date.now();
    if (
      !forceRefresh &&
      cached !== null &&
      this.maxAgeMs > 0 &&
      now - cached.fetchedAt < this.maxAgeMs
    ) {
      return cached.catalog;
    }

    const headers: Record<string, string> = {
      Accept: "application/json",
      "User-Agent": this.userAgent,
    };
    if (cached?.etag) {
      headers["If-None-Match"] = cached.etag;
    }

    try {
      const resp = await this.transport.fetchCatalog(
        this.endpoint,
        headers,
        this.requestTimeoutMs
      );
      if (resp.status === 304 && cached !== null) {
        const etag = resp.etag ?? cached.etag;
        const ts = Date.now();
        this.memory = { catalog: cached.catalog, fetchedAt: ts, etag };
        await this.storage?.updateMetadata(etag, ts);
        return cached.catalog;
      }
      if (resp.status < 200 || resp.status > 299) {
        throw new ModelCatalogError("unexpected_status", `HTTP ${resp.status}`, {
          statusCode: resp.status,
        });
      }
      const catalog = decodeCatalog(resp.body);
      const ts = Date.now();
      const etag = resp.etag ?? null;
      this.memory = { catalog, fetchedAt: ts, etag };
      await this.storage?.write(resp.body, etag, ts);
      return catalog;
    } catch (err) {
      if (cached !== null) return cached.catalog;
      if (err instanceof ModelCatalogError) throw err;
      throw new ModelCatalogError("unavailable", String(err), { cause: err });
    }
  }
}
