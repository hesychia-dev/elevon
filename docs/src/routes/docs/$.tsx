import { createFileRoute, Link, notFound } from "@tanstack/react-router";
import { createServerFn } from "@tanstack/react-start";
import { staticFunctionMiddleware } from "@tanstack/start-static-server-functions";
import { useFumadocsLoader } from "fumadocs-core/source/client";
import { DocsLayout } from "fumadocs-ui/layouts/spacious";
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
  MarkdownCopyButton,
  ViewOptionsPopover,
} from "fumadocs-ui/layouts/spacious/page";
import { type ReactNode, Suspense, use, useEffect } from "react";
import { useMDXComponents } from "@/components/mdx";
import { baseOptions } from "@/lib/layout.shared";
import { getPageImageUrl, getPageMarkdownUrl, gitConfig } from "@/lib/shared";
import { docs, source } from "@/lib/source";

function scrollToLocationHash() {
  const hash = window.location.hash.slice(1);
  if (!hash) return;

  const el = document.getElementById(decodeURIComponent(hash));
  el?.scrollIntoView({ behavior: "smooth", block: "start" });
}

function renderInlineCode(text: string): ReactNode {
  const nodes: ReactNode[] = [];
  let lastIndex = 0;

  for (const match of text.matchAll(/(`[^`]+`)/g)) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index));
    }

    nodes.push(
      <code
        key={`code-${match.index}`}
        className="rounded-md border border-fd-border bg-fd-muted px-1.5 py-0.5 font-mono text-[0.8125rem] text-fd-foreground"
      >
        {match[1].slice(1, -1)}
      </code>,
    );
    lastIndex = match.index + match[0].length;
  }

  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }

  return nodes;
}

export const Route = createFileRoute("/docs/$")({
  component: Page,
  loader: async ({ params }) => {
    const slugs = params._splat?.split("/") ?? [];
    const data = await loader({ data: slugs });
    await docs.getPage(data.path)?.preload();
    return data;
  },
  head: ({ loaderData }) => ({
    meta: loaderData
      ? [{ property: "og:image", content: loaderData.imageUrl }]
      : [],
  }),
});

const loader = createServerFn({
  method: "GET",
})
  .validator((slugs: string[]) => slugs)
  .middleware([staticFunctionMiddleware])
  .handler(async ({ data: slugs }) => {
    const page = source.getPage(slugs);
    if (!page) throw notFound();

    return {
      path: page.path,
      markdownUrl: getPageMarkdownUrl(page).url,
      imageUrl: getPageImageUrl(page).url,
      pageTree: await source.serializePageTree(source.getPageTree()),
    };
  });

function Content({ path, markdownUrl }: { path: string; markdownUrl: string }) {
  const page = docs.getPage(path);
  if (!page) throw new Error(`unknown page: ${path}`);

  const { toc } = use(page.load());
  const MDX = page.body;

  // Hash scroll runs before Suspense MDX mounts; retry once content is painted.
  useEffect(() => {
    if (!path) return;
    scrollToLocationHash();
  }, [path]);

  return (
    <DocsPage toc={toc}>
      <DocsTitle>{page.title}</DocsTitle>
      <DocsDescription>
        {page.description ? renderInlineCode(page.description) : null}
      </DocsDescription>
      <div className="flex flex-row gap-2 items-center border-b -mt-4 pb-6">
        <MarkdownCopyButton markdownUrl={markdownUrl} />
        <ViewOptionsPopover
          markdownUrl={markdownUrl}
          githubUrl={`https://github.com/${gitConfig.user}/${gitConfig.repo}/blob/${gitConfig.branch}/content/docs/${path}`}
        />
      </div>
      <DocsBody>
        <MDX components={useMDXComponents()} />
      </DocsBody>
    </DocsPage>
  );
}

function Page() {
  const { pageTree, path, markdownUrl } = useFumadocsLoader(
    Route.useLoaderData(),
  );

  return (
    <DocsLayout {...baseOptions()} tree={pageTree}>
      <Link to={markdownUrl} hidden />
      <Suspense>
        <Content path={path} markdownUrl={markdownUrl} />
      </Suspense>
    </DocsLayout>
  );
}
