import { loginFn } from "@/actions/auth.functions";
import { createFormAction } from "@/lib/action";

export const loginAction = createFormAction(loginFn);
