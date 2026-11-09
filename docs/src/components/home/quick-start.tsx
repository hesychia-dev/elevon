import { useCopyButton } from "fumadocs-ui/utils/use-copy-button";
import { Check, Clipboard } from "lucide-react";

export function QuickStart() {
  return (
    <section id="quickstart" className="border-t border-fd-border px-6 py-20">
      <div className="mx-auto max-w-5xl">
        <h2 className="mb-3 text-2xl font-semibold tracking-tight text-fd-foreground">
          Install in minutes
        </h2>
        <p className="mb-10 max-w-2xl text-fd-muted-foreground">
          Put the agent on the target VM and the CLI on your development
          machine.
        </p>

        <div className="flex min-w-0 flex-col gap-6">
          <div className="min-w-0">
            <div className="mb-3 flex flex-wrap items-center gap-2">
              <p className="text-sm font-medium text-fd-foreground">CLI</p>
              <PlatformBadge>Linux x86_64</PlatformBadge>
              <PlatformBadge>macOS aarch64</PlatformBadge>
            </div>
            <CopyableCode text="curl -fsSL https://elevon.ccxlv.dev/get/cli.sh | bash" />
          </div>
          <div className="min-w-0">
            <div className="mb-3 flex flex-wrap items-center gap-2">
              <p className="text-sm font-medium text-fd-foreground">Agent</p>
              <PlatformBadge>Linux x86_64</PlatformBadge>
            </div>
            <CopyableCode text="curl -fsSL https://elevon.ccxlv.dev/get/agent.sh | sudo bash" />
          </div>
        </div>
      </div>
    </section>
  );
}

function PlatformBadge({ children }: { children: string }) {
  return (
    <span className="rounded-md border border-fd-primary/25 bg-fd-primary/10 px-2 py-0.5 font-mono text-[0.7rem] tracking-wide text-fd-primary">
      {children}
    </span>
  );
}

function CopyableCode({ text }: { text: string }) {
  const [copied, onCopy] = useCopyButton(() =>
    navigator.clipboard.writeText(text),
  );

  return (
    <div className="flex min-w-0 items-start gap-2 rounded-xl border border-fd-border bg-fd-card text-fd-card-foreground">
      <pre className="min-w-0 flex-1 overflow-x-auto whitespace-pre-wrap break-all p-4 text-[0.8125rem] leading-relaxed">
        <code>{text}</code>
      </pre>
      <button
        type="button"
        onClick={onCopy}
        aria-label={copied ? "Copied" : "Copy"}
        className="mt-2 me-2 inline-flex size-8 shrink-0 items-center justify-center rounded-md text-fd-muted-foreground transition hover:bg-fd-accent hover:text-fd-accent-foreground"
      >
        {copied ? (
          <Check className="size-3.5" />
        ) : (
          <Clipboard className="size-3.5" />
        )}
      </button>
    </div>
  );
}
