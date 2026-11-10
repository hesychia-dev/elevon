import { Link } from "@tanstack/react-router";
import {
  BookOpen,
  Container,
  GitBranch,
  Rocket,
  Server,
  Terminal,
} from "lucide-react";

export function DocsLinks() {
  const links = [
    {
      href: "",
      title: "Introduction",
      body: "What Elevon is and how the pieces fit together.",
      icon: BookOpen,
    },
    {
      href: "install",
      title: "Installation",
      body: "Put the agent on your VM and the CLI on your laptop.",
      icon: Container,
    },
    {
      href: "agent",
      title: "Agent",
      body: "Host daemon config, routing, deploys, and operations.",
      icon: Server,
    },
    {
      href: "cli",
      title: "CLI",
      body: "Deploy config, build, push, and ship releases.",
      icon: Terminal,
    },
    {
      href: "deploy",
      title: "First deploy",
      body: "From empty host to a live release.",
      icon: Rocket,
    },
    {
      href: "ci",
      title: "GitHub Actions",
      body: "Run elevon deploy from CI.",
      icon: GitBranch,
    },
  ] as const;

  return (
    <section className="border-t border-fd-border px-6 py-20">
      <div className="mx-auto max-w-5xl">
        <h2 className="mb-3 text-2xl font-semibold tracking-tight text-fd-foreground">
          Jump into the docs
        </h2>
        <p className="mb-10 max-w-2xl text-fd-muted-foreground">
          Guides and references for shipping with Elevon.
        </p>

        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {links.map((item) => {
            const Icon = item.icon;
            return (
              <Link
                key={item.title}
                to="/docs/$"
                params={{ _splat: item.href }}
                className="group rounded-2xl border border-fd-border bg-fd-background p-5 transition hover:border-fd-primary/40 hover:bg-fd-primary/5"
              >
                <Icon className="mb-4 size-5 text-fd-primary" />
                <h3 className="mb-1 font-semibold text-fd-foreground group-hover:text-fd-primary">
                  {item.title}
                </h3>
                <p className="text-sm text-fd-muted-foreground">{item.body}</p>
              </Link>
            );
          })}
        </div>
      </div>
    </section>
  );
}
