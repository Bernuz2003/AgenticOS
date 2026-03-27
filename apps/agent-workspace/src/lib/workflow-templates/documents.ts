import { buildStructuredTaskPrompt, type WorkflowTemplateDefinition } from "./catalog";

export const documentWorkflowTemplates: WorkflowTemplateDefinition[] = [
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
          constraints: ["Avoid dumping every file name if it is not useful."],
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
];
