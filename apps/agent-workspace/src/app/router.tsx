import { createHashRouter } from "react-router-dom";
import { AppLayout } from "./layout";
import { LobbyPage } from "../pages/lobby-page";
import { WorkspacePage } from "../pages/workspace-page";

export const appRouter = createHashRouter([
  {
    path: "/",
    element: <AppLayout />,
    children: [
      {
        index: true,
        element: <LobbyPage />,
      },
      {
        path: "workspace/:sessionId",
        element: <WorkspacePage />,
      },
    ],
  },
]);
