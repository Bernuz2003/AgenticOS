import { ArrowRight } from "lucide-react";
import { Link } from "react-router-dom";

import { BuilderPane } from "./builder-pane";
import { TemplateGallery } from "./template-gallery";
import { TaskListEditor } from "./task-list-editor";
import { useWorkflowBuilder } from "./hooks/useWorkflowBuilder";

export function WorkflowsPage() {
  const workflowBuilder = useWorkflowBuilder();

  return (
    <div className="mx-auto max-w-7xl space-y-8">
      <header className="overflow-hidden rounded-[32px] border border-slate-200 bg-white shadow-sm">
        <div className="bg-[radial-gradient(circle_at_top_left,_rgba(99,102,241,0.14),_transparent_48%),linear-gradient(135deg,_rgba(248,250,252,1),_rgba(255,255,255,0.96))] px-8 py-8">
          <div className="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
            <div className="max-w-3xl">
              <div className="text-xs font-bold uppercase tracking-[0.28em] text-slate-400">
                Workflow Studio
              </div>
              <h1 className="mt-3 text-3xl font-bold tracking-tight text-slate-900">
                Templates and builder stay here. Execution moves to Jobs.
              </h1>
              <p className="mt-3 max-w-2xl text-sm leading-6 text-slate-600">
                `Chats` remain conversational. `Workflows` is now the design surface:
                choose a template, customize the DAG and optionally attach a scheduler.
                Live runs and scheduled jobs are monitored in a dedicated runtime view.
              </p>
            </div>
            <div className="flex flex-wrap gap-3">
              <Link
                to="/sessions"
                className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
              >
                Go to Chats
              </Link>
              <Link
                to="/jobs"
                className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-5 py-2.5 text-sm font-semibold text-white hover:bg-slate-800"
              >
                Open Jobs
                <ArrowRight className="h-4 w-4" />
              </Link>
            </div>
          </div>

          <div className="mt-8 inline-flex rounded-2xl border border-slate-200 bg-white p-1 shadow-sm">
            <button
              type="button"
              onClick={() => workflowBuilder.setView("templates")}
              className={`rounded-xl px-4 py-2 text-sm font-semibold transition ${
                workflowBuilder.view === "templates"
                  ? "bg-indigo-50 text-indigo-700"
                  : "text-slate-600 hover:text-slate-900"
              }`}
            >
              Templates
            </button>
            <button
              type="button"
              onClick={() => workflowBuilder.setView("builder")}
              className={`rounded-xl px-4 py-2 text-sm font-semibold transition ${
                workflowBuilder.view === "builder"
                  ? "bg-indigo-50 text-indigo-700"
                  : "text-slate-600 hover:text-slate-900"
              }`}
            >
              Builder
            </button>
          </div>
        </div>
      </header>

      {workflowBuilder.view === "templates" ? (
        <TemplateGallery
          categories={workflowBuilder.categories}
          filteredTemplates={workflowBuilder.filteredTemplates}
          selectedTemplateId={workflowBuilder.selectedTemplateId}
          selectedTemplate={workflowBuilder.selectedTemplate}
          templateQuery={workflowBuilder.templateQuery}
          templateCategory={workflowBuilder.templateCategory}
          onTemplateQueryChange={workflowBuilder.setTemplateQuery}
          onTemplateCategoryChange={workflowBuilder.setTemplateCategory}
          onSelectTemplate={workflowBuilder.setSelectedTemplateId}
          onApplyTemplate={workflowBuilder.applyTemplate}
        />
      ) : (
        <div className="grid gap-6 xl:grid-cols-[minmax(0,1.15fr)_360px]">
          <BuilderPane
            failurePolicy={workflowBuilder.failurePolicy}
            onFailurePolicyChange={workflowBuilder.setFailurePolicy}
            draftSourceTemplateName={workflowBuilder.draftSourceTemplateName}
            tasks={workflowBuilder.tasks}
            selectedTaskIndex={workflowBuilder.selectedTaskIndex}
            onSelectTask={workflowBuilder.setSelectedTaskIndex}
            onTasksChange={workflowBuilder.setTasks}
            onAddTask={workflowBuilder.addTask}
            onRemoveTask={workflowBuilder.removeTask}
            schedulerDraft={workflowBuilder.schedulerDraft}
            setSchedulerDraft={workflowBuilder.setSchedulerDraft}
            submitError={workflowBuilder.submitError}
          />
          <TaskListEditor
            tasks={workflowBuilder.tasks}
            rootTasksCount={workflowBuilder.rootTasks.length}
            failurePolicy={workflowBuilder.failurePolicy}
            schedulerDraft={workflowBuilder.schedulerDraft}
            validation={workflowBuilder.validation}
            selectedTask={workflowBuilder.selectedTask}
            selectedTaskIndex={workflowBuilder.selectedTaskIndex}
            onUpdateTask={workflowBuilder.updateTask}
            submittingMode={workflowBuilder.submittingMode}
            onLaunchWorkflow={workflowBuilder.handleLaunchWorkflow}
            onScheduleWorkflow={workflowBuilder.handleScheduleWorkflow}
            onResetBuilder={workflowBuilder.resetBuilder}
          />
        </div>
      )}
    </div>
  );
}
