import {
  Container,
  FileStack,
  Folder,
  KeyRound,
  type LucideIcon,
  Network,
  RotateCcw,
  Server,
  Terminal,
  Workflow,
} from "lucide-react";
import {
  Children,
  isValidElement,
  type ReactElement,
  type ReactNode,
} from "react";

const icons = {
  terminal: Terminal,
  server: Server,
  network: Network,
  workflow: Workflow,
  files: FileStack,
  container: Container,
  key: KeyRound,
  rollback: RotateCcw,
  folder: Folder,
} satisfies Record<string, LucideIcon>;

export function WelcomeLead({ children }: { children: ReactNode }) {
  return (
    <p className="not-prose mb-10 text-lg leading-relaxed text-fd-muted-foreground">
      {children}
    </p>
  );
}

export function FlowSteps({
  children,
  note,
}: {
  children: ReactNode;
  note?: ReactNode;
}) {
  const steps = Children.toArray(children).filter(
    isValidElement,
  ) as ReactElement<{
    children: ReactNode;
  }>[];

  return (
    <div className="not-prose my-8 overflow-hidden rounded-2xl border border-fd-border">
      <ol className="m-0 list-none p-0">
        {steps.map((step, index) => (
          <li
            key={Children.toArray(step.props.children).join("")}
            className="grid grid-cols-[3rem_1fr] gap-3 border-b border-fd-border px-4 py-4 last:border-b-0"
          >
            <span className="font-mono text-sm font-semibold text-fd-primary tabular-nums">
              {String(index + 1).padStart(2, "0")}
            </span>
            <p className="text-[0.9375rem] leading-relaxed text-fd-foreground">
              {step.props.children}
            </p>
          </li>
        ))}
      </ol>
      {note ? (
        <p className="border-t border-fd-border bg-fd-muted/40 px-4 py-3.5 text-[0.9375rem] leading-relaxed text-fd-muted-foreground">
          {note}
        </p>
      ) : null}
    </div>
  );
}

export function FlowStep({ children }: { children: ReactNode }) {
  return <>{children}</>;
}

export function InfoGrid({ children }: { children: ReactNode }) {
  return (
    <div className="not-prose my-8 grid gap-3 sm:grid-cols-2">{children}</div>
  );
}

export function InfoCard({
  title,
  meta,
  icon,
  tone = "default",
  children,
}: {
  title: string;
  meta?: string;
  icon?: keyof typeof icons;
  tone?: "default" | "accent";
  children: ReactNode;
}) {
  const Icon = icon ? icons[icon] : null;

  return (
    <div
      className={
        tone === "accent"
          ? "rounded-2xl border border-fd-primary/15 bg-fd-primary/5 p-5"
          : "rounded-2xl border border-fd-border bg-fd-card p-5"
      }
    >
      {Icon || meta ? (
        <div className="mb-4 flex items-center justify-between gap-3">
          {Icon ? (
            <span
              className={
                tone === "accent"
                  ? "text-fd-primary"
                  : "inline-flex size-10 items-center justify-center rounded-xl bg-fd-primary text-fd-primary-foreground shadow-sm shadow-fd-primary/25"
              }
            >
              <Icon className="size-5" />
            </span>
          ) : (
            <span />
          )}
          {meta ? (
            <span className="font-mono text-[0.7rem] tracking-wide text-fd-primary uppercase">
              {meta}
            </span>
          ) : null}
        </div>
      ) : null}
      <h3 className="mb-2 text-lg font-semibold tracking-tight text-fd-primary">
        {title}
      </h3>
      <p className="text-[0.9375rem] leading-relaxed text-fd-muted-foreground [&_a]:underline [&_a]:underline-offset-2">
        {children}
      </p>
    </div>
  );
}

export function FitSplit({ children }: { children: ReactNode }) {
  return (
    <div className="not-prose my-8 grid gap-3 lg:grid-cols-2">{children}</div>
  );
}

export function FitColumn({
  title,
  tone = "yes",
  children,
}: {
  title: string;
  tone?: "yes" | "no";
  children: ReactNode;
}) {
  return (
    <div
      className={
        tone === "yes"
          ? "rounded-2xl border border-fd-border bg-fd-card p-5"
          : "rounded-2xl border border-fd-border bg-fd-muted/40 p-5"
      }
    >
      <p
        className={
          tone === "yes"
            ? "mb-4 font-mono text-[0.7rem] tracking-[0.14em] text-fd-primary uppercase"
            : "mb-4 font-mono text-[0.7rem] tracking-[0.14em] text-fd-muted-foreground uppercase"
        }
      >
        {title}
      </p>
      <ul className="m-0 list-none space-y-3 p-0">{children}</ul>
    </div>
  );
}

export function FitItem({
  tone = "yes",
  children,
}: {
  tone?: "yes" | "no";
  children: ReactNode;
}) {
  return (
    <li className="flex gap-3 text-[0.9375rem] leading-relaxed">
      <span
        className={
          tone === "yes"
            ? "mt-2 size-1.5 shrink-0 rounded-full bg-fd-primary"
            : "mt-2 size-1.5 shrink-0 rounded-full bg-fd-muted-foreground/50"
        }
      />
      <span
        className={
          tone === "yes" ? "text-fd-foreground" : "text-fd-muted-foreground"
        }
      >
        {children}
      </span>
    </li>
  );
}

export function RequirementsPanel({
  pieceLabel = "Piece",
  needLabel = "Need",
  children,
}: {
  pieceLabel?: string;
  needLabel?: string;
  children: ReactNode;
}) {
  return (
    <div className="not-prose my-8 overflow-hidden rounded-2xl border border-fd-border">
      <div className="grid grid-cols-[8rem_1fr] border-b border-fd-border bg-fd-muted/50 px-4 py-2.5 font-mono text-[0.7rem] tracking-wide text-fd-muted-foreground uppercase sm:grid-cols-[10rem_1fr]">
        <span>{pieceLabel}</span>
        <span>{needLabel}</span>
      </div>
      {children}
    </div>
  );
}

export function Requirement({
  piece,
  children,
}: {
  piece: string;
  children: ReactNode;
}) {
  return (
    <div className="grid grid-cols-[8rem_1fr] items-center gap-x-2 border-b border-fd-border bg-fd-card px-4 py-3.5 last:border-b-0 sm:grid-cols-[10rem_1fr]">
      <span className="text-sm font-semibold text-fd-primary">{piece}</span>
      <span className="text-[0.9375rem] text-fd-muted-foreground [&_code]:rounded-md [&_code]:border [&_code]:border-fd-border [&_code]:bg-fd-muted [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.8125rem] [&_code]:text-fd-foreground">
        {children}
      </span>
    </div>
  );
}
