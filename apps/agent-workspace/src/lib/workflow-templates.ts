import {
  initialSchedulerDraft,
  type DraftBackendClass,
  type DraftContextStrategy,
  type DraftTask,
  type DraftWorkload,
  type FailurePolicy,
  type SchedulerDraft,
} from "./workflow-builder";

interface WorkflowTemplateTask {
  id: string;
  role: string;
  prompt: string;
  deps?: string[];
  workload?: Exclude<DraftWorkload, "">;
  backendClass?: Exclude<DraftBackendClass, "">;
  contextStrategy?: Exclude<DraftContextStrategy, "">;
  contextWindowSize?: number;
}

export interface WorkflowTemplateDefinition {
  id: string;
  name: string;
  category: string;
  description: string;
  summary: string;
  tags: string[];
  recommendedRuntime: string;
  failurePolicy: FailurePolicy;
  tasks: WorkflowTemplateTask[];
  schedulerPreset?: Partial<SchedulerDraft>;
}

export const workflowTemplates: WorkflowTemplateDefinition[] = [
  {
    id: "planner-worker-reviewer",
    name: "Planner -> Worker -> Reviewer",
    category: "Coding",
    description:
      "Classic structured execution for scoped implementation or research tasks with a final review gate.",
    summary: "Plan the work, execute it, then critique and tighten the result.",
    tags: ["Recommended", "Coding", "Review"],
    recommendedRuntime: "Mixed reasoning/general",
    failurePolicy: "fail_fast",
    tasks: [
      {
        id: "plan",
        role: "Planner",
        prompt:
          "Break the request into a concise executable plan. Surface assumptions, constraints and acceptance criteria.",
        workload: "reasoning",
        contextStrategy: "retrieve",
      },
      {
        id: "implement",
        role: "Worker",
        prompt:
          "Execute the plan and produce the main deliverable. Keep it concrete and aligned with the accepted scope.",
        deps: ["plan"],
        workload: "code",
        contextStrategy: "retrieve",
      },
      {
        id: "review",
        role: "Reviewer",
        prompt:
          "Review the output for correctness, omissions, regressions and clarity. Produce a final corrected result if needed.",
        deps: ["implement"],
        workload: "reasoning",
        contextStrategy: "summarize",
      },
    ],
  },
  {
    id: "research-pipeline",
    name: "Research Pipeline",
    category: "Research",
    description:
      "Multi-step research flow that separates discovery, synthesis and final briefing.",
    summary: "Scout sources, distill evidence, then deliver a clean executive brief.",
    tags: ["Research", "Analysis"],
    recommendedRuntime: "Reasoning-heavy local/cloud",
    failurePolicy: "best_effort",
    tasks: [
      {
        id: "scout",
        role: "Scout",
        prompt:
          "Collect the most relevant facts, examples and conflicting evidence related to the request.",
        workload: "reasoning",
        contextStrategy: "retrieve",
      },
      {
        id: "synthesize",
        role: "Synthesizer",
        prompt:
          "Consolidate the discovered evidence into a structured view with agreements, disagreements and open gaps.",
        deps: ["scout"],
        workload: "reasoning",
        contextStrategy: "summarize",
      },
      {
        id: "brief",
        role: "Briefing Writer",
        prompt:
          "Produce the final briefing with findings, risks and recommended next actions.",
        deps: ["synthesize"],
        workload: "general",
        contextStrategy: "summarize",
      },
    ],
  },
  {
    id: "multi-model-compare",
    name: "Multi-Model Compare",
    category: "Compare",
    description:
      "Run the same request through parallel tracks and finish with a synthesis/comparison layer.",
    summary: "Produce multiple independent answers, then compare quality, style and tradeoffs.",
    tags: ["Compare", "Models", "Evaluation"],
    recommendedRuntime: "Parallel mixed backends",
    failurePolicy: "best_effort",
    tasks: [
      {
        id: "candidate_a",
        role: "Candidate A",
        prompt:
          "Answer the request independently with a high-quality solution. Focus on correctness and completeness.",
        workload: "reasoning",
        contextStrategy: "sliding_window",
      },
      {
        id: "candidate_b",
        role: "Candidate B",
        prompt:
          "Answer the request independently with a different approach or framing. Surface different tradeoffs where possible.",
        workload: "reasoning",
        contextStrategy: "sliding_window",
      },
      {
        id: "compare",
        role: "Comparator",
        prompt:
          "Compare the upstream candidates, identify which parts are strongest and produce a merged final recommendation.",
        deps: ["candidate_a", "candidate_b"],
        workload: "reasoning",
        contextStrategy: "retrieve",
      },
    ],
  },
  {
    id: "codebase-analyst",
    name: "Codebase Analyst",
    category: "Coding",
    description:
      "Useful for repo inspection, architectural review or impact analysis before implementation.",
    summary: "Map the codebase, inspect hotspots and deliver a concrete technical report.",
    tags: ["Coding", "Analysis", "Review"],
    recommendedRuntime: "Code-aware local",
    failurePolicy: "fail_fast",
    tasks: [
      {
        id: "map_repo",
        role: "Mapper",
        prompt:
          "Inspect the repository structure and identify the most relevant modules, boundaries and data flows for the request.",
        workload: "code",
        contextStrategy: "retrieve",
      },
      {
        id: "inspect_hotspots",
        role: "Analyst",
        prompt:
          "Inspect the relevant modules in depth. Find likely bugs, risks, coupling points and missing invariants.",
        deps: ["map_repo"],
        workload: "code",
        contextStrategy: "retrieve",
      },
      {
        id: "report",
        role: "Reporter",
        prompt:
          "Produce a technical report with findings, impacted files, risks and recommended next actions.",
        deps: ["inspect_hotspots"],
        workload: "general",
        contextStrategy: "summarize",
      },
    ],
  },
  {
    id: "document-digest",
    name: "Document Digest",
    category: "Analysis",
    description:
      "Digest a folder or document set into summaries, key points and follow-up questions.",
    summary: "Summarize dense material without losing its key signals.",
    tags: ["Documents", "Analysis", "Digest"],
    recommendedRuntime: "General local",
    failurePolicy: "best_effort",
    tasks: [
      {
        id: "read_docs",
        role: "Reader",
        prompt:
          "Read the provided documents and extract key sections, repeated motifs and important factual anchors.",
        workload: "general",
        contextStrategy: "retrieve",
      },
      {
        id: "summarize",
        role: "Summarizer",
        prompt:
          "Produce a concise but complete digest, including key takeaways, open questions and action items.",
        deps: ["read_docs"],
        workload: "general",
        contextStrategy: "summarize",
      },
    ],
  },
  {
    id: "summarize-directory",
    name: "Summarize Directory",
    category: "Analysis",
    description:
      "Quickly summarize a directory or project subtree with structure, purpose and critical files.",
    summary: "Useful when onboarding into an unfamiliar workspace or code package.",
    tags: ["Directory", "Analysis"],
    recommendedRuntime: "Fast local",
    failurePolicy: "fail_fast",
    tasks: [
      {
        id: "inventory",
        role: "Inventory",
        prompt:
          "List the directory contents and group files by responsibility, importance and likely entry points.",
        workload: "fast",
        contextStrategy: "retrieve",
      },
      {
        id: "summary",
        role: "Summarizer",
        prompt:
          "Produce a directory summary with important files, responsibilities and probable next places to inspect.",
        deps: ["inventory"],
        workload: "general",
        contextStrategy: "summarize",
      },
    ],
  },
  {
    id: "spec-plan-critic",
    name: "Spec -> Plan -> Critic",
    category: "Planning",
    description:
      "Turns a loose request into a spec, operational plan and critique pass.",
    summary: "Ideal for scoping before implementation or delegation.",
    tags: ["Planning", "Review"],
    recommendedRuntime: "Reasoning local/cloud",
    failurePolicy: "fail_fast",
    tasks: [
      {
        id: "spec",
        role: "Specifier",
        prompt:
          "Convert the request into a crisp technical spec with constraints, assumptions and acceptance criteria.",
        workload: "reasoning",
        contextStrategy: "retrieve",
      },
      {
        id: "plan",
        role: "Planner",
        prompt:
          "Produce a concrete execution plan based on the specification.",
        deps: ["spec"],
        workload: "reasoning",
        contextStrategy: "summarize",
      },
      {
        id: "critic",
        role: "Critic",
        prompt:
          "Critique the specification and plan for blind spots, regressions, cost and complexity.",
        deps: ["plan"],
        workload: "reasoning",
        contextStrategy: "summarize",
      },
    ],
  },
  {
    id: "background-report-job",
    name: "Background Report Job",
    category: "Automation",
    description:
      "A ready-made template for scheduled recurring reports with retry and timeout defaults.",
    summary: "Designed for jobs that should run unattended on a schedule.",
    tags: ["Automation", "Scheduler", "Reports"],
    recommendedRuntime: "General background worker",
    failurePolicy: "best_effort",
    tasks: [
      {
        id: "collect",
        role: "Collector",
        prompt:
          "Collect the latest state relevant to the requested report and prepare structured findings.",
        workload: "general",
        contextStrategy: "retrieve",
      },
      {
        id: "report",
        role: "Reporter",
        prompt:
          "Produce the final report with highlights, deltas, anomalies and follow-up recommendations.",
        deps: ["collect"],
        workload: "general",
        contextStrategy: "summarize",
      },
    ],
    schedulerPreset: {
      name: "background_report",
      triggerKind: "cron",
      cronExpression: "0 8 * * 1-5",
      timeoutSeconds: "1200",
      maxRetries: "2",
      backoffSeconds: "120",
      enabled: true,
    },
  },
];

export function workflowTemplateCategories(): string[] {
  return Array.from(
    new Set(workflowTemplates.map((template) => template.category)),
  ).sort();
}

export function instantiateTemplateDraft(
  template: WorkflowTemplateDefinition,
): {
  failurePolicy: FailurePolicy;
  tasks: DraftTask[];
  schedulerDraft: SchedulerDraft;
} {
  const schedulerDraft = {
    ...initialSchedulerDraft(),
    ...template.schedulerPreset,
  };

  return {
    failurePolicy: template.failurePolicy,
    schedulerDraft,
    tasks: template.tasks.map((task) => ({
      id: task.id,
      role: task.role,
      prompt: task.prompt,
      depsText: (task.deps ?? []).join(", "),
      workload: task.workload ?? "",
      backendClass: task.backendClass ?? "",
      contextStrategy: task.contextStrategy ?? "",
      contextWindowSize: task.contextWindowSize
        ? String(task.contextWindowSize)
        : "",
    })),
  };
}
