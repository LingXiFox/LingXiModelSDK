export type ModelCatalogErrorCode =
  | "invalid_catalog"
  | "unsupported_schema_version"
  | "unexpected_status"
  | "transport"
  | "unavailable";

export class ModelCatalogError extends Error {
  readonly code: ModelCatalogErrorCode;
  readonly statusCode: number | undefined;
  readonly schemaVersion: string | undefined;

  constructor(
    code: ModelCatalogErrorCode,
    message: string,
    options?: { statusCode?: number; schemaVersion?: string; cause?: unknown }
  ) {
    super(message, options?.cause !== undefined ? { cause: options.cause } : undefined);
    this.name = "ModelCatalogError";
    this.code = code;
    this.statusCode = options?.statusCode;
    this.schemaVersion = options?.schemaVersion;
  }
}
