import { deleteCookie, setCookie } from "@tanstack/react-start/server";

export const setSessionCookie = (token: string) => {
  setCookie("session", token, {
    path: "/",
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
  });
};

export const clearSessionCookie = () => {
  deleteCookie("session", { path: "/" });
};
