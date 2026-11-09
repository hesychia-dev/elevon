import defaultMdxComponents from "fumadocs-ui/mdx";
import type { MDXComponents } from "mdx/types";
import {
  FitColumn,
  FitItem,
  FitSplit,
  FlowStep,
  FlowSteps,
  InfoCard,
  InfoGrid,
  Requirement,
  RequirementsPanel,
  WelcomeLead,
} from "@/components/welcome";
import { WhyItem, WhyList } from "@/components/why-list";

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    WhyList,
    WhyItem,
    WelcomeLead,
    FlowSteps,
    FlowStep,
    InfoGrid,
    InfoCard,
    FitSplit,
    FitColumn,
    FitItem,
    RequirementsPanel,
    Requirement,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}
