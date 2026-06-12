// Types for Cerul Cloud (accounts.cerul.ai). Mirrors the server's public
// contracts; kept in-app because cerul-cloud is a separate private repo.

export type CloudPlan = "free" | "pro" | "team" | "enterprise";

export interface CloudUser {
  id: string;
  email: string;
  name: string;
  plan: CloudPlan;
  status: "active" | "disabled";
  email_verified: boolean;
  created_at: string;
}

export interface AuthResponse {
  user: CloudUser;
  access_token: string;
  token_type: "Bearer";
  expires_at: string;
}

export interface RegisterInput {
  email: string;
  password: string;
  name?: string;
}

export interface LoginInput {
  email: string;
  password: string;
}

export interface OAuthExchangeInput {
  code: string;
  state: string;
}

// Thrown for non-2xx responses; carries the server's error code + HTTP status
// so callers can branch (e.g. 401 → clear session, "email_already_registered").
export class CloudApiError extends Error {
  constructor(
    public readonly code: string,
    message: string,
    public readonly status: number,
  ) {
    super(message);
    this.name = "CloudApiError";
  }
}
