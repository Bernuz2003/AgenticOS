import { createElement } from "react";
import { createHashRouter } from "react-router-dom";
import { AppLayout } from "./layout";
import { DashboardPage } from "./page";
import { JobsPage } from "../pages/jobs/page";
import { SessionsPage } from "../pages/chats/page";
import { ModelsPage } from "../pages/models/page";
import { ControlCenterPage, ResourcesPage } from "../pages/resources/page";
import { SettingsPage } from "../pages/settings/page";
import { WorkflowRunPage } from "../pages/workflow-run/page";
import { WorkflowsPage } from "../pages/workflows/page";
import { WorkspacePage } from "../pages/chats/detail";

export const appRouter = createHashRouter([
  {
    path: "/",
    element: createElement(AppLayout),
    children: [
      {
        index: true,
        element: createElement(DashboardPage),
      },
      {
        path: "sessions",
        element: createElement(SessionsPage),
      },
      {
        path: "workflows",
        element: createElement(WorkflowsPage),
      },
      {
        path: "jobs",
        element: createElement(JobsPage),
      },
      {
        path: "workflow-runs/:orchestrationId",
        element: createElement(WorkflowRunPage),
      },
      {
        path: "models",
        element: createElement(ModelsPage),
      },
      {
        path: "settings",
        element: createElement(SettingsPage),
      },
      {
        path: "control-center",
        element: createElement(ControlCenterPage),
      },
      {
        path: "resources",
        element: createElement(ResourcesPage),
      },
      {
        path: "workspace/:sessionId",
        element: createElement(WorkspacePage),
      },
    ],
  },
]);
