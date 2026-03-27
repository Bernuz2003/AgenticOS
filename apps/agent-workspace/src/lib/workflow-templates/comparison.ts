import { buildStructuredTaskPrompt, type WorkflowTemplateDefinition } from "./catalog";

export const comparisonWorkflowTemplates: WorkflowTemplateDefinition[] = [
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
];
