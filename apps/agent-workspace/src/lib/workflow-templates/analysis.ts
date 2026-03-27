import { buildStructuredTaskPrompt, type WorkflowTemplateDefinition } from "./catalog";

export const analysisWorkflowTemplates: WorkflowTemplateDefinition[] = [
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
          constraints: ["Do not start planning yet."],
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
          inputs: ["Use the spec artifact as the authoritative problem definition."],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Work Breakdown",
            "2. Execution Order",
            "3. Risks",
            "4. Validation Steps",
          ],
          constraints: ["Keep the plan operational and testable."],
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
          inputs: ["Use the spec and plan artifacts together."],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Blind Spots",
            "2. Regression Risks",
            "3. Complexity Concerns",
            "4. Recommended Adjustments",
          ],
          constraints: ["Be specific about what should change and why."],
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
          constraints: ["Prefer deltas and anomalies over generic status prose."],
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
          inputs: ["Use the collector artifact as the authoritative source."],
          outputLines: [
            "Return Markdown with exactly these sections:",
            "[Result Artifact]",
            "1. Highlights",
            "2. Notable Deltas",
            "3. Anomalies",
            "4. Follow-up Recommendations",
          ],
          constraints: ["Keep the report concise and ready to send as-is."],
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
