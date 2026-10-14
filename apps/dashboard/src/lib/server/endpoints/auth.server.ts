import type { LoginDto } from "@/lib/schema/auth.ts";
import { http } from "@/lib/server/http.server.ts";
import type {
  LoginResponse,
  User,
} from "@/lib/server/types/auth.type.server.ts";

export const login = (payload: LoginDto) =>
  http.post<LoginResponse>("/auth/login", payload);

export const fetchUser = () => http.get<User>("/auth/me");

export const logout = () => http.post<void>("/auth/logout");
