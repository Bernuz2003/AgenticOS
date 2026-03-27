import { buildStructuredTaskPrompt, type WorkflowTemplateDefinition } from "./catalog";

export const codingWorkflowTemplates: WorkflowTemplateDefinition[] = [
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
];
