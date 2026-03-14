import { createHashRouter } from "react-router-dom";
import { AppLayout } from "./layout";
import { DashboardPage } from "../pages/dashboard-page";
import { SessionsPage } from "../pages/sessions-page";
import { ResourcesPage } from "../pages/resources-page";
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
