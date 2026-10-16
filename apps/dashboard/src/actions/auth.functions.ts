import { createServerFn } from "@tanstack/react-start";
import { clearSessionCookie, setSessionCookie } from "@/actions/auth.server";
import { type ActionState, actionFail, actionOk } from "@/lib/action";
import { parseLoginForm } from "@/lib/schema/auth";
import { fetchUser, login, logout } from "@/lib/server/endpoints/mod.server";

export const loginFn = createServerFn({ method: "POST" })
  .validator(parseLoginForm)
  .handler(async ({ data }): Promise<ActionState> => {
    const result = await login(data);
    if (!result.ok) {
      return actionFail(result.error.message, result.error.code);
    }

    setSessionCookie(result.data.sessionToken);
    return actionOk();
  });

export const getCurrentUserFn = createServerFn({ method: "GET" }).handler(
  async () => {
    const result = await fetchUser();
    return result.ok ? result.data : null;
  },
);

export const logoutFn = createServerFn({ method: "POST" }).handler(async () => {
  await logout();
  clearSessionCookie();
});
