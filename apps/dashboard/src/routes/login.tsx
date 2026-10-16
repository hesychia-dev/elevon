import { createFileRoute, redirect } from "@tanstack/react-router";
import { useActionState, useEffect } from "react";
import { loginAction } from "@/actions/auth.actions";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { safeRedirectPath } from "@/lib/utils";
import { userQueryOptions } from "@/queries/auth";

export const Route = createFileRoute("/login")({
  head: () => ({
    meta: [{ title: "Login - elevon Dashboard" }],
  }),
  validateSearch: (search: Record<string, unknown>): { redirect?: string } => {
    if (typeof search.redirect === "string") {
      return { redirect: search.redirect };
    }
    return {};
  },
  beforeLoad: async ({ context, search }) => {
    const user = await context.queryClient.ensureQueryData(userQueryOptions);

    if (user) {
      throw redirect({ href: safeRedirectPath(search.redirect) });
    }
  },
  component: RouteComponent,
});

function RouteComponent() {
  const { redirect } = Route.useSearch();

  const [state, formAction, isPending] = useActionState(loginAction, {
    status: "idle",
  });

  useEffect(() => {
    if (state.status === "success") {
      globalThis.location.href = safeRedirectPath(redirect);
    }
  }, [state, redirect]);

  return (
    <div className="flex min-h-svh items-center justify-center px-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle className="font-heading text-2xl">Sign in</CardTitle>
          <CardDescription>
            Enter the shared access key to open the console.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form action={formAction} className="flex flex-col gap-4">
            <div className="flex flex-col gap-2">
              <Label htmlFor="key">Access key</Label>
              <Input
                id="key"
                name="key"
                type="password"
                placeholder="••••••••"
                autoComplete="off"
                required
              />
            </div>
            <Button type="submit" className="w-full" disabled={isPending}>
              Sign in
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
