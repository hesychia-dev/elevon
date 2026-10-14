import { queryOptions } from "@tanstack/react-query";
import { getCurrentUserFn } from "@/actions/auth.functions";
import { queryKeys } from "./keys.ts";

export const userQueryOptions = queryOptions({
  queryKey: queryKeys.auth.user,
  queryFn: () => getCurrentUserFn(),
  staleTime: 60_000,
});
