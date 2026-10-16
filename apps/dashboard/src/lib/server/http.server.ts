import { getCookies } from "@tanstack/react-start/server";
import type { ApiError, ApiResult } from "@/lib/api-error.ts";

const API_BASE_URL = Deno.env.get("API_URL");

function fail(error: ApiError): ApiResult<never> {
  console.error(`[api] ${error.status} ${error.code}: ${error.message}`);
  return { ok: false, error };
}

async function request<T>(
  path: string,
  method: string,
  requestBody?: unknown,
): Promise<ApiResult<T>> {
  const url = `${API_BASE_URL}${path}`;
  const headers = new Headers();
  const cookies = getCookies();

  let body: BodyInit | null = null;
  if (requestBody !== undefined) {
    headers.set("Content-Type", "application/json");
    body = JSON.stringify(requestBody);
  }

  if (cookies.session) {
    headers.set("Authorization", `Bearer ${cookies.session}`);
  }

  let response: Response;
  try {
    response = await fetch(url, {
      method,
      headers,
      body,
    });
  } catch (err) {
    return fail({
      status: 0,
      code: "network",
      message: err instanceof Error ? err.message : "Network error",
    });
  }

  if (!response.ok) {
    const payload = await response.json().catch(() => null);

    return fail({
      status: response.status,
      code: typeof payload?.code === "string" ? payload.code : "unknown",
      message:
        typeof payload?.message === "string"
          ? payload.message
          : `API Error: ${response.status}`,
    });
  }

  if (response.status === 204) {
    return { ok: true, data: undefined as T };
  }

  return { ok: true, data: (await response.json()) as T };
}

export const http = {
  get: <T>(path: string) => request<T>(path, "GET"),

  post: <T>(path: string, body?: unknown) => request<T>(path, "POST", body),

  put: <T>(path: string, body?: unknown) => request<T>(path, "PUT", body),

  delete: <T>(path: string) => request<T>(path, "DELETE"),
};
