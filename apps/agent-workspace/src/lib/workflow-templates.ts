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

interface PromptSpec {
  objective: string;
  inputs: string[];
  outputLines: string[];
  constraints?: string[];
}

const BASE_RESULT_CONSTRAINTS = [
  "Do the work directly; do not narrate that you will do it.",
  "Do not include planning chatter, repeated tool intentions or transcript-style commentary in the final result.",
  "If tools are needed, use them, then return only the durable result artifact.",
  "Keep the result information-dense and easy for the next task to consume.",
];

function bulletList(items: string[]): string {
  return items.map((item) => `- ${item}`).join("\n");
}

function buildStructuredTaskPrompt(spec: PromptSpec): string {
  return [
    "[Objective]",
    spec.objective,
    "",
    "[Input Contract]",
    bulletList(spec.inputs),
    "",
    "[Output Contract]",
    ...spec.outputLines,
    "",
    "[Behavior Constraints]",
    bulletList([...BASE_RESULT_CONSTRAINTS, ...(spec.constraints ?? [])]),
  ].join("\n");
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
        prompt: buildStructuredTaskPrompt({
          objective:
            "Turn the workflow request into a concrete execution plan that another task can follow without guessing.",
          inputs: [
            "Assume the workflow input contains the user objective, scope and any constraints.",
            "Use only the information provided by the workflow input and upstream artifacts.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Scope",
            "2. Assumptions",
            "3. Execution Steps",
            "4. Acceptance Criteria",
            "5. Risks",
          ],
          constraints: [
            "Do not write implementation code in this task.",
            "Do not repeat the original request verbatim unless needed to clarify scope.",
          ],
        }),
        workload: "reasoning",
        contextStrategy: "retrieve",
      },
      {
        id: "implement",
        role: "Worker",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Execute the approved plan and produce the main deliverable for the workflow objective.",
          inputs: [
            "Use the planner artifact as the authoritative execution plan.",
            "If upstream artifacts include constraints or acceptance criteria, satisfy them explicitly.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Deliverable",
            "2. Key Decisions",
            "3. Outstanding Gaps",
          ],
          constraints: [
            "Do not restate the plan unless a step materially changes.",
            "If a tool is required, use it once per needed operation and work from the acquired result.",
          ],
        }),
        deps: ["plan"],
        workload: "code",
        contextStrategy: "retrieve",
      },
      {
        id: "review",
        role: "Reviewer",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Review the worker deliverable for correctness, omissions, regressions and clarity, then produce the corrected final version if needed.",
          inputs: [
            "Use the worker artifact as the primary subject of review.",
            "Use the planner artifact only to verify scope and acceptance criteria.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Verdict",
            "2. Confirmed Strengths",
            "3. Issues To Fix",
            "4. Corrected Final Result",
          ],
          constraints: [
            "Be explicit about defects instead of vague criticism.",
            "If the worker result is already good, keep the corrected final result concise and complete.",
          ],
        }),
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
        prompt: buildStructuredTaskPrompt({
          objective:
            "Collect the most relevant evidence, facts, examples and disagreements related to the workflow research question.",
          inputs: [
            "Assume the workflow input defines the research topic and scope.",
            "Only include evidence that is materially useful for downstream synthesis.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Confirmed Findings",
            "2. Conflicting Evidence",
            "3. Important Examples",
            "4. Missing Information",
          ],
          constraints: [
            "Avoid generic background filler.",
            "Do not produce the final briefing in this step.",
          ],
        }),
        workload: "reasoning",
        contextStrategy: "retrieve",
      },
      {
        id: "synthesize",
        role: "Synthesizer",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Consolidate the research evidence into a structured analysis that clearly separates strong conclusions from uncertainty.",
          inputs: [
            "Use the scout artifact as the evidence pack.",
            "Preserve contradictions and open gaps instead of flattening them away.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Core Conclusions",
            "2. Agreements",
            "3. Disagreements",
            "4. Confidence Notes",
            "5. Open Gaps",
          ],
          constraints: [
            "Do not re-list every raw fact if it does not change the conclusion.",
            "Make the evidence hierarchy clear.",
          ],
        }),
        deps: ["scout"],
        workload: "reasoning",
        contextStrategy: "summarize",
      },
      {
        id: "brief",
        role: "Briefing Writer",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Produce the final briefing for the end user from the synthesized analysis.",
          inputs: [
            "Use the synthesizer artifact as the authoritative analysis.",
            "Optimize for fast comprehension and actionable next steps.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Executive Summary",
            "2. Key Findings",
            "3. Risks And Caveats",
            "4. Recommended Next Actions",
          ],
          constraints: [
            "Do not include raw evidence dumps.",
            "Keep the briefing concise but decision-ready.",
          ],
        }),
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
        prompt: buildStructuredTaskPrompt({
          objective:
            "Produce an independent high-quality answer to the workflow objective using one coherent approach.",
          inputs: [
            "Treat the workflow input as the full assignment.",
            "Do not assume knowledge of the other candidate output.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Answer",
            "2. Main Tradeoffs",
          ],
          constraints: [
            "Keep the answer self-contained.",
            "Do not mention that another candidate exists.",
          ],
        }),
        workload: "reasoning",
        contextStrategy: "sliding_window",
      },
      {
        id: "candidate_b",
        role: "Candidate B",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Produce an independent answer that uses a materially different framing, angle or tradeoff profile from a typical default solution.",
          inputs: [
            "Treat the workflow input as the full assignment.",
            "Do not assume knowledge of the other candidate output.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Answer",
            "2. Main Tradeoffs",
          ],
          constraints: [
            "Surface a different angle rather than superficial wording changes.",
            "Keep the answer self-contained.",
          ],
        }),
        workload: "reasoning",
        contextStrategy: "sliding_window",
      },
      {
        id: "compare",
        role: "Comparator",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Compare the independent candidates, identify the strongest parts of each and produce one merged recommendation.",
          inputs: [
            "Use both candidate artifacts as the comparison corpus.",
            "Prefer concrete strengths and weaknesses over generic scoring.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Comparison Matrix",
            "2. Best Elements To Keep",
            "3. Final Merged Recommendation",
          ],
          constraints: [
            "Do not preserve duplicated content unless it adds value.",
            "Explain why the final merged recommendation is better than either candidate alone.",
          ],
        }),
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
        prompt: buildStructuredTaskPrompt({
          objective:
            "Inspect the repository structure and identify the modules, boundaries and data flows that matter for the workflow objective.",
          inputs: [
            "Assume the workflow input contains the repo path and the analysis objective.",
            "Use repository inspection tools only as needed to build the map.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Relevant Areas",
            "2. Key Files",
            "3. Data / Control Flows",
            "4. Initial Questions",
          ],
          constraints: [
            "Do not start deep bug analysis in this task.",
            "Avoid exhaustive file listings that do not help the objective.",
          ],
        }),
        workload: "code",
        contextStrategy: "retrieve",
      },
      {
        id: "inspect_hotspots",
        role: "Analyst",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Inspect the mapped hotspots in depth and find concrete bugs, risks, coupling points and missing invariants.",
          inputs: [
            "Use the mapper artifact to decide where to inspect deeply.",
            "Only include issues that are technically grounded in the inspected code.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Findings",
            "2. Impacted Files",
            "3. Architectural Risks",
            "4. Missing Invariants / Tests",
          ],
          constraints: [
            "Prefer precise findings over generic maintainability complaints.",
            "Do not rewrite the repository map unless it changes a finding.",
          ],
        }),
        deps: ["map_repo"],
        workload: "code",
        contextStrategy: "retrieve",
      },
      {
        id: "report",
        role: "Reporter",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Produce the final technical report from the hotspot analysis.",
          inputs: [
            "Use the analyst artifact as the authoritative findings set.",
            "Optimize for an engineer who needs to act on the report.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Executive Summary",
            "2. Ranked Findings",
            "3. Risks",
            "4. Recommended Next Actions",
          ],
          constraints: [
            "Keep the report concrete and action-oriented.",
            "Do not pad the report with restated repository context.",
          ],
        }),
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
        prompt: buildStructuredTaskPrompt({
          objective:
            "Read the workflow input document set and extract a clean evidence pack for downstream summarization.",
          inputs: [
            "Assume the workflow input specifies the file, folder or document set to inspect.",
            "If you need to read files, read each required target once, then work from the acquired content.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Documents Read",
            "2. Key Sections",
            "3. Repeated Themes",
            "4. Factual Anchors",
            "5. Open Ambiguities",
          ],
          constraints: [
            "Do not narrate tool usage.",
            "Do not repeat the same read_file request after the content is already available.",
            "Do not produce the final digest in this step.",
          ],
        }),
        workload: "general",
        contextStrategy: "retrieve",
      },
      {
        id: "summarize",
        role: "Summarizer",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Produce the final digest from the extracted document evidence pack.",
          inputs: [
            "Use the reader artifact as the authoritative source.",
            "Focus on the most decision-relevant information from the documents.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Executive Digest",
            "2. Key Takeaways",
            "3. Important Facts",
            "4. Open Questions",
            "5. Suggested Follow-ups",
          ],
          constraints: [
            "Do not re-read the documents if the reader artifact already contains the needed content.",
            "Do not include transcript fragments or meta-commentary.",
          ],
        }),
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
        prompt: buildStructuredTaskPrompt({
          objective:
            "Inspect a directory or subtree and produce a structured inventory of what matters.",
          inputs: [
            "Assume the workflow input specifies the directory path and any focus area.",
            "Only include files and folders that matter for understanding the target area.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Major Areas",
            "2. Important Files",
            "3. Entry Points",
            "4. Questions For Further Inspection",
          ],
          constraints: [
            "Avoid dumping every file name if it is not useful.",
          ],
        }),
        workload: "fast",
        contextStrategy: "retrieve",
      },
      {
        id: "summary",
        role: "Summarizer",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Produce the final directory summary from the structured inventory.",
          inputs: [
            "Use the inventory artifact as the primary source.",
            "Optimize for someone onboarding quickly into the target area.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Purpose Overview",
            "2. Important Files And Responsibilities",
            "3. Suggested Next Inspection Steps",
          ],
          constraints: [
            "Do not restate the entire inventory if a shorter summary is enough.",
          ],
        }),
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
        prompt: buildStructuredTaskPrompt({
          objective:
            "Convert the workflow objective into a crisp technical specification.",
          inputs: [
            "Assume the workflow input contains the request, target domain and any explicit constraints.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Objective",
            "2. Constraints",
            "3. Assumptions",
            "4. Acceptance Criteria",
          ],
          constraints: [
            "Do not start planning yet.",
          ],
        }),
        workload: "reasoning",
        contextStrategy: "retrieve",
      },
      {
        id: "plan",
        role: "Planner",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Produce a concrete execution plan based on the approved specification.",
          inputs: [
            "Use the spec artifact as the authoritative problem definition.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Work Breakdown",
            "2. Execution Order",
            "3. Risks",
            "4. Validation Steps",
          ],
          constraints: [
            "Keep the plan operational and testable.",
          ],
        }),
        deps: ["spec"],
        workload: "reasoning",
        contextStrategy: "summarize",
      },
      {
        id: "critic",
        role: "Critic",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Critique the specification and execution plan for blind spots, regressions, unnecessary complexity and cost.",
          inputs: [
            "Use the spec and plan artifacts together.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Blind Spots",
            "2. Regression Risks",
            "3. Complexity Concerns",
            "4. Recommended Adjustments",
          ],
          constraints: [
            "Be specific about what should change and why.",
          ],
        }),
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
        prompt: buildStructuredTaskPrompt({
          objective:
            "Collect the state needed for a recurring operational report and structure it for downstream reporting.",
          inputs: [
            "Assume the workflow input defines the subject of the scheduled report and the time horizon to compare.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Current State",
            "2. Deltas Since Last Run",
            "3. Anomalies",
            "4. Items Requiring Attention",
          ],
          constraints: [
            "Prefer deltas and anomalies over generic status prose.",
          ],
        }),
        workload: "general",
        contextStrategy: "retrieve",
      },
      {
        id: "report",
        role: "Reporter",
        prompt: buildStructuredTaskPrompt({
          objective:
            "Produce the final unattended report from the collected operational findings.",
          inputs: [
            "Use the collector artifact as the authoritative source.",
          ],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Highlights",
            "2. Notable Deltas",
            "3. Anomalies",
            "4. Follow-up Recommendations",
          ],
          constraints: [
            "Keep the report concise and ready to send as-is.",
          ],
        }),
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
    })),
  };
}
