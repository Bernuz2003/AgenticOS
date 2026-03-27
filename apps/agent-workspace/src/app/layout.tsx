import { Outlet } from "react-router-dom";
import { ShellLayout } from "../components/shell/layout";
import { useKernelEvents } from "../hooks/useKernelEvents";

export function AppLayout() {
  useKernelEvents();

  return <ShellLayout><Outlet /></ShellLayout>;
}
