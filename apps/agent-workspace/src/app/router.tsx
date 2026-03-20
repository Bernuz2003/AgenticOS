import { createHashRouter } from "react-router-dom";
import { AppLayout } from "./layout";
import { DashboardPage } from "../pages/dashboard-page";
import { JobsPage } from "../pages/jobs-page";
import { SessionsPage } from "../pages/sessions-page";
import { ControlCenterPage, ResourcesPage } from "../pages/resources-page";
import { WorkflowRunPage } from "../pages/workflow-run-page";
import { WorkflowsPage } from "../pages/workflows-page";
import { WorkspacePage } from "../pages/workspace-page";

export const appRouter = createHashRouter([
  {
    path: "/",
    element: <AppLayout />,
    children: [
      {
        index: true,
        element: <DashboardPage />,
      },
      {
        path: "sessions",
        element: <SessionsPage />,
      },
      {
        path: "workflows",
        element: <WorkflowsPage />,
      },
      {
        path: "jobs",
        element: <JobsPage />,
      },
      {
        path: "workflow-runs/:orchestrationId",
        element: <WorkflowRunPage />,
      },
      {
        path: "control-center",
        element: <ControlCenterPage />,
      },
      {
        path: "resources",
        element: <ResourcesPage />,
      },
      {
        path: "workspace/:sessionId",
        element: <WorkspacePage />,
      },
    ],
  },
]);
