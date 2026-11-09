import type { ReactNode } from "react";

export function WhyList({ children }: { children: ReactNode }) {
  return <ul className="not-prose my-6 list-none space-y-3 p-0">{children}</ul>;
}

export function WhyItem({ children }: { children: ReactNode }) {
  return (
    <li className="flex gap-3 text-[0.9375rem] leading-relaxed text-fd-foreground">
      <span
        aria-hidden
        className="mt-2 size-1.5 shrink-0 rounded-full bg-fd-primary shadow-[0_0_0_3px] shadow-fd-primary/20"
      />
      <span>{children}</span>
    </li>
  );
}
