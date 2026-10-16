import type { QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  HeadContent,
  Outlet,
  Scripts,
} from "@tanstack/react-router";
import type { ReactNode } from "react";
import { Providers } from "@/components/providers";
import "@/styles/app.css";

const SHELL_BG = "oklch(0.145 0 0)";

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()(
  {
    head: () => ({
      meta: [
        { charSet: "utf-8" },
        {
          name: "viewport",
          content: "width=device-width, initial-scale=1",
        },
        { title: "elevon Dashboard" },
      ],
    }),
    component: RootComponent,
    notFoundComponent: () => <p>Not found</p>,
  },
);

function RootComponent() {
  return (
    <RootDocument>
      <Providers>
        <Outlet />
      </Providers>
    </RootDocument>
  );
}

function RootDocument({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <html
      lang="en"
      className="dark"
      style={{ colorScheme: "dark", backgroundColor: SHELL_BG }}
    >
      <head>
        <HeadContent />
      </head>
      <body
        className="min-h-svh antialiased"
        style={{ backgroundColor: SHELL_BG, color: "#fafafa" }}
      >
        {children}
        <Scripts />
      </body>
    </html>
  );
}
