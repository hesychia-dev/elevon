import { createFileRoute } from "@tanstack/react-router";
import { HomeLayout } from "fumadocs-ui/layouts/home";
import { Capabilities } from "@/components/home/capabilities";
import { DocsLinks } from "@/components/home/docs-links";
import { Hero } from "@/components/home/hero";
import { QuickStart } from "@/components/home/quick-start";
import { Workflow } from "@/components/home/workflow";
import { baseOptions } from "@/lib/layout.shared";

export const Route = createFileRoute("/")({
  component: Home,
});

function Home() {
  return (
    <HomeLayout {...baseOptions()}>
      <main className="flex min-w-0 flex-1 flex-col overflow-x-clip">
        <Hero />
        <Workflow />
        <Capabilities />
        <QuickStart />
        <DocsLinks />
      </main>
    </HomeLayout>
  );
}
