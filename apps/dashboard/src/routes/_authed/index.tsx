import { useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { logoutFn } from "@/actions/auth.functions";
import { Button } from "@/components/ui/button";
import { queryKeys } from "@/queries/keys";

export const Route = createFileRoute("/_authed/")({
  component: RouteComponent,
});

function RouteComponent() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  return (
    <div className="mx-auto flex min-h-svh items-center justify-center px-4 py-8">
      <Button
        type="button"
        variant="outline"
        onClick={async () => {
          await logoutFn();
          queryClient.setQueryData(queryKeys.auth.user, null);
          await navigate({ to: "/login" });
        }}
      >
        Logout
      </Button>
    </div>
  );
}
