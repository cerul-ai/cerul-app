import {
  CloudApiError,
  type AuthResponse,
  type CloudUser,
  type CreateShareInput,
  type LoginInput,
  type OAuthExchangeInput,
  type RegisterInput,
  type PublishedShareResponse,
  type ShareDraftResponse,
} from "./types";

// Cerul Cloud account API. Distinct from the local core (lib/api.ts).
export const CLOUD_API_BASE_URL = "https://accounts.cerul.ai";

interface RequestOptions {
  method?: "GET" | "POST" | "DELETE";
  token?: string;
  body?: unknown;
}

async function upload(path: string, token: string, body: Blob) {
  const isAbsolute = /^https?:\/\//i.test(path);
  const target = isAbsolute ? path : `${CLOUD_API_BASE_URL}${path}`;
  let response: Response;
  try {
    response = await fetch(target, {
      method: "PUT",
      headers: isAbsolute
        ? { "content-type": body.type || "application/octet-stream" }
        : {
            authorization: `Bearer ${token}`,
            "content-type": body.type || "application/octet-stream",
          },
      body,
    });
  } catch {
    throw new CloudApiError("network_error", "could not reach Cerul Cloud", 0);
  }
  if (!response.ok) {
    const data = (await response.json().catch(() => null)) as { error?: { code?: string; message?: string } } | null;
    throw new CloudApiError(data?.error?.code ?? "upload_failed", data?.error?.message ?? `upload failed (${response.status})`, response.status);
  }
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = {};
  if (options.body !== undefined) {
    headers["content-type"] = "application/json";
  }
  if (options.token) {
    headers.authorization = `Bearer ${options.token}`;
  }

  let response: Response;
  try {
    response = await fetch(`${CLOUD_API_BASE_URL}${path}`, {
      method: options.method ?? "GET",
      headers,
      ...(options.body !== undefined ? { body: JSON.stringify(options.body) } : {}),
    });
  } catch {
    // Network failure (offline, DNS). Surface as a recognizable error.
    throw new CloudApiError("network_error", "could not reach Cerul Cloud", 0);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  const data = (await response.json().catch(() => null)) as unknown;
  if (!response.ok) {
    const error = (data as { error?: { code?: string; message?: string } } | null)?.error;
    throw new CloudApiError(
      error?.code ?? "unknown_error",
      error?.message ?? `request failed (${response.status})`,
      response.status,
    );
  }
  return data as T;
}

export const cloudClient = {
  oauthStartUrl(provider: "google" | "github") {
    const redirect = encodeURIComponent("cerul-app://auth/callback");
    return `${CLOUD_API_BASE_URL}/v1/auth/oauth/${provider}/start?redirect_uri=${redirect}`;
  },
  register(input: RegisterInput) {
    return request<AuthResponse>("/v1/auth/register", { method: "POST", body: input });
  },
  login(input: LoginInput) {
    return request<AuthResponse>("/v1/auth/login", { method: "POST", body: input });
  },
  exchangeOAuth(input: OAuthExchangeInput) {
    return request<AuthResponse>("/v1/auth/oauth/exchange", { method: "POST", body: input });
  },
  logout(token: string) {
    return request<void>("/v1/auth/logout", { method: "POST", token });
  },
  me(token: string) {
    return request<{ user: CloudUser }>("/v1/me", { token });
  },
  sendVerificationCode(token: string) {
    return request<{ sent: boolean }>("/v1/auth/email/send-code", { method: "POST", token });
  },
  verifyEmail(token: string, code: string) {
    return request<{ user: CloudUser }>("/v1/auth/email/verify", { method: "POST", token, body: { code } });
  },
  createShare(token: string, input: CreateShareInput) {
    return request<ShareDraftResponse>("/v1/shares", { method: "POST", token, body: input });
  },
  uploadShareMedia(token: string, path: string, body: Blob) {
    return upload(path, token, body);
  },
  publishShare(token: string, shareId: string) {
    return request<PublishedShareResponse>(`/v1/shares/${encodeURIComponent(shareId)}/publish`, { method: "POST", token });
  },
  revokeShare(token: string, shareId: string) {
    return request<void>(`/v1/shares/${encodeURIComponent(shareId)}`, { method: "DELETE", token });
  },
};
