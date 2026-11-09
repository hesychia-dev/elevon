export function Workflow() {
  const steps = [
    {
      title: "Build",
      body: "elevon deploy builds your container image locally from the deploy config.",
    },
    {
      title: "Publish",
      body: "The same command pushes the image to your registry.",
    },
    {
      title: "Deploy",
      body: "The agent pulls the image, starts the new release, and shuts the old one down gracefully so requests keep flowing.",
    },
    {
      title: "Rollback",
      body: "Restore a previous release when a deploy needs to come back down.",
    },
  ];

  return (
    <section className="border-t border-fd-border px-6 py-20">
      <div className="mx-auto max-w-5xl">
        <h2 className="mb-3 text-2xl font-semibold tracking-tight text-fd-foreground">
          Source to running service
        </h2>
        <p className="mb-10 max-w-2xl text-fd-muted-foreground">
          One fast deploy: build and push on the client, then the agent rolls
          the release with a graceful handoff.
        </p>

        <ol className="grid gap-8 sm:grid-cols-2 lg:grid-cols-4">
          {steps.map((step, index) => (
            <li key={step.title} className="relative">
              <p className="mb-2 font-mono text-xs tracking-wide text-fd-primary">
                {String(index + 1).padStart(2, "0")}
              </p>
              <h3 className="mb-2 text-base font-semibold text-fd-foreground">
                {step.title}
              </h3>
              <p className="text-sm leading-relaxed text-fd-muted-foreground">
                {step.body}
              </p>
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}
