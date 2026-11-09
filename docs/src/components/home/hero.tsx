import { Link } from "@tanstack/react-router";
import { ArrowRight } from "lucide-react";

export function Hero() {
  return (
    <section className="relative overflow-hidden px-6 pb-20 pt-16 sm:pb-28 sm:pt-24">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_70%_55%_at_50%_-10%,rgba(245,180,20,0.22),transparent_60%)]"
      />
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 bg-[linear-gradient(to_right,color-mix(in_oklab,var(--color-fd-border)_70%,transparent)_1px,transparent_1px),linear-gradient(to_bottom,color-mix(in_oklab,var(--color-fd-border)_70%,transparent)_1px,transparent_1px)] bg-size-[48px_48px] mask-[radial-gradient(ellipse_70%_60%_at_50%_20%,black,transparent)] opacity-35"
      />

      <div className="relative mx-auto grid max-w-6xl items-center gap-12 lg:grid-cols-[1.05fr_0.95fr] lg:gap-16">
        <div className="min-w-0 animate-[elevon-rise_0.7s_ease-out]">
          <p className="mb-5 text-sm font-medium tracking-[0.18em] text-fd-primary uppercase">
            Self-hosted deployments
          </p>
          <h1 className="mb-2 text-5xl font-semibold tracking-tight text-fd-foreground sm:text-6xl lg:text-7xl">
            Elevon
          </h1>
          <p className="mb-4 text-xl font-medium tracking-tight text-fd-foreground/85 sm:text-2xl">
            Fast deploys. Traffic keeps flowing.
          </p>
          <p className="mb-9 max-w-lg text-base leading-relaxed text-fd-muted-foreground sm:text-lg">
            Build and push your container image from your machine. The agent
            takes it from there, switching releases without dropping traffic.
          </p>
          <div className="flex flex-wrap items-center gap-3">
            <Link
              to="/docs/$"
              params={{ _splat: "" }}
              className="inline-flex items-center gap-2 rounded-lg bg-fd-primary px-4 py-2.5 text-sm font-medium text-fd-primary-foreground shadow-lg shadow-fd-primary/25 transition hover:brightness-110"
            >
              Read the docs
              <ArrowRight className="size-4" />
            </Link>
            <a
              href="#quickstart"
              className="inline-flex items-center gap-2 rounded-lg border border-fd-border bg-fd-background/80 px-4 py-2.5 text-sm font-medium text-fd-foreground backdrop-blur-sm transition hover:bg-fd-accent"
            >
              Quick start
            </a>
          </div>
        </div>

        <HeroPanel />
      </div>
    </section>
  );
}

function HeroPanel() {
  return (
    <div className="relative min-w-0 animate-[elevon-rise_0.95s_ease-out]">
      <div
        aria-hidden
        className="absolute -inset-6 rounded-4xl bg-fd-primary/10 blur-2xl animate-[elevon-glow_8s_ease-in-out_infinite]"
      />

      <div className="relative overflow-hidden rounded-2xl border border-fd-border bg-fd-card/90 shadow-xl shadow-fd-primary/5 backdrop-blur-sm">
        <div className="flex items-center gap-2 border-b border-fd-border px-4 py-3">
          <span className="size-2.5 rounded-full bg-fd-border" />
          <span className="size-2.5 rounded-full bg-fd-border" />
          <span className="size-2.5 rounded-full bg-fd-primary/70" />
          <span className="ms-2 font-mono text-[0.7rem] text-fd-muted-foreground">
            elevon deploy
          </span>
        </div>

        <div className="space-y-3 p-4 sm:p-5">
          <p className="font-mono text-[0.75rem] text-fd-foreground">
            <span className="text-fd-primary">$</span> elevon deploy
          </p>

          <HeroStage
            side="CLI"
            title="On your machine"
            lines={[
              "→ building image locally",
              "→ pushing image to registry",
              "→ secured request to the agent",
            ]}
          />

          <div className="flex items-center gap-3 px-1">
            <div className="h-px flex-1 bg-linear-to-r from-transparent via-fd-primary/50 to-transparent" />
            <span className="font-mono text-[0.65rem] tracking-wide text-fd-primary uppercase">
              same command
            </span>
            <div className="h-px flex-1 bg-linear-to-r from-transparent via-fd-primary/50 to-transparent" />
          </div>

          <HeroStage
            side="Agent"
            title="On the VM"
            lines={[
              "← receiving deploy request",
              "← pulling image from registry",
              "← starting new release",
              "← graceful shutdown of old process",
              "✓ traffic kept serving",
            ]}
          />
        </div>
      </div>
    </div>
  );
}

function HeroStage({
  side,
  title,
  lines,
}: {
  side: string;
  title: string;
  lines: string[];
}) {
  return (
    <div className="rounded-xl border border-fd-border/80 bg-fd-background/70 p-3.5">
      <div className="mb-3 flex items-center justify-between gap-3">
        <p className="text-base font-semibold tracking-tight text-fd-foreground">
          {title}
        </p>
        <span className="rounded-md border border-fd-primary/25 bg-fd-primary/10 px-2 py-0.5 font-mono text-[0.65rem] tracking-wide text-fd-primary">
          {side}
        </span>
      </div>
      <pre className="overflow-x-auto whitespace-pre-wrap break-all font-mono text-[0.75rem] leading-relaxed text-fd-muted-foreground">
        <code>
          {lines.map((line, i) => (
            <span key={line}>
              {i > 0 ? "\n" : null}
              <span
                className={
                  line.startsWith("$")
                    ? "text-fd-foreground"
                    : line.startsWith("✓")
                      ? "text-fd-primary"
                      : undefined
                }
              >
                {line}
              </span>
            </span>
          ))}
        </code>
      </pre>
    </div>
  );
}
