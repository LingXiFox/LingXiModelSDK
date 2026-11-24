import { createHash, createHmac } from "node:crypto";

function sha256Hex(data: string | Uint8Array): string {
  return createHash("sha256").update(data).digest("hex");
}

function hmacSha256(key: string | Uint8Array, data: string): Buffer {
  return createHmac("sha256", key).update(data).digest();
}

export function signSigV4(params: {
  method: string;
  canonicalURI: string;
  canonicalQuery?: string;
  host: string;
  region: string;
  service: string;
  accessKey: string;
  secretKey: string;
  sessionToken?: string;
  payload: string;
  now?: Date;
}): Record<string, string> {
  const now = params.now ?? new Date();
  const amzDate = now.toISOString().replace(/[:-]|\.\d{3}/g, "");
  const dateStamp = amzDate.slice(0, 8);
  const payloadHash = sha256Hex(params.payload);

  const hdrs: Record<string, string> = {
    host: params.host,
    "x-amz-date": amzDate,
  };
  if (params.sessionToken) {
    hdrs["x-amz-security-token"] = params.sessionToken;
  }
  const keys = Object.keys(hdrs).sort();
  const canonicalHeaders = keys.map((k) => `${k}:${hdrs[k].trim()}\n`).join("");
  const signedHeaders = keys.join(";");

  const canonicalReq = [
    params.method,
    params.canonicalURI,
    params.canonicalQuery ?? "",
    canonicalHeaders,
    signedHeaders,
    payloadHash,
  ].join("\n");

  const scope = `${dateStamp}/${params.region}/${params.service}/aws4_request`;
  const stringToSign = ["AWS4-HMAC-SHA256", amzDate, scope, sha256Hex(canonicalReq)].join("\n");

  const kDate = hmacSha256(`AWS4${params.secretKey}`, dateStamp);
  const kRegion = hmacSha256(kDate, params.region);
  const kService = hmacSha256(kRegion, params.service);
  const kSigning = hmacSha256(kService, "aws4_request");
  const signature = hmacSha256(kSigning, stringToSign).toString("hex");

  const out: Record<string, string> = {
    Authorization: `AWS4-HMAC-SHA256 Credential=${params.accessKey}/${scope}, SignedHeaders=${signedHeaders}, Signature=${signature}`,
    "X-Amz-Date": amzDate,
  };
  if (params.sessionToken) {
    out["X-Amz-Security-Token"] = params.sessionToken;
  }
  return out;
}
