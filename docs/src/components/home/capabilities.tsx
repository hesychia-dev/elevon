import {
  FileStack,
  Network,
  Timer,
  Workflow as WorkflowIcon,
} from "lucide-react";

export function Capabilities() {
  const items = [
    {
      title: "Graceful handoffs",
      body: "Web servers keep serving during deploys. Old processes shut down gracefully after the new release is ready.",
      icon: Timer,
    },
    {
      title: "Workers",
      body: "Run background workers with no domain routing. Same image, different role.",
      icon: WorkflowIcon,
    },
    {
      title: "Custom ports & TLS",
      body: "Bind any port, with TLS or without. Use your own certs when you need HTTPS.",
      icon: Network,
    },
    {
      title: "Static apps",
      body: "Ship apps that are just files. Serve a built directory without a long-running server process.",
      icon: FileStack,
    },
  ] as const;

  return (
    <section className="border-t border-fd-border bg-fd-muted/40 px-6 py-20">
      <div className="mx-auto max-w-5xl">
        <h2 className="mb-3 text-2xl font-semibold tracking-tight text-fd-foreground">
          Built for real production shapes
        </h2>
        <p className="mb-10 max-w-2xl text-fd-muted-foreground">
          Fast releases for web apps, workers, and static sites on your own
          hosts.
        </p>

        <ul className="grid gap-8 sm:grid-cols-2">
          {items.map((item) => {
            const Icon = item.icon;
            return (
              <li key={item.title} className="min-w-0">
                <div className="mb-3 inline-flex size-9 items-center justify-center rounded-lg bg-fd-primary/15 text-fd-primary">
                  <Icon className="size-4.5" />
                </div>
                <h3 className="mb-2 text-base font-semibold text-fd-foreground">
                  {item.title}
                </h3>
                <p className="text-sm leading-relaxed text-fd-muted-foreground">
                  {item.body}
                </p>
              </li>
            );
          })}
        </ul>
      </div>
    </section>
  );
}
