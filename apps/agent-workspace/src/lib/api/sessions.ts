import { invoke } from "@tauri-apps/api/core";

import type {
  SendInputResult,
  StartSessionResult,
  TurnControlResult,
} from "./index";

export async function startSession(
  prompt: string,
  workload: string,
): Promise<StartSessionResult> {
  const session = await invoke<{
    session_id: string;
    pid: number;
  }>("start_session", { prompt, workload });

  return {
    sessionId: session.session_id,
    pid: session.pid,
  };
}

export async function resumeSession(
  sessionId: string,
): Promise<StartSessionResult> {
  const session = await invoke<{
    session_id: string;
    pid: number;
  }>("resume_session", { sessionId });

  return {
    sessionId: session.session_id,
    pid: session.pid,
  };
}

export async function sendSessionInput(payload: {
  pid?: number | null;
  sessionId?: string | null;
  prompt: string;
}): Promise<SendInputResult> {
  const result = await invoke<{
    pid: number;
    state: string;
  }>("send_session_input", {
    pid: payload.pid ?? null,
    sessionId: payload.sessionId ?? null,
    prompt: payload.prompt,
  });

  return {
    pid: result.pid,
    state: result.state,
  };
}

export async function continueSessionOutput(
  pid: number,
): Promise<TurnControlResult> {
  const result = await invoke<{
    pid: number;
    state: string;
    action: string;
  }>("continue_session_output", { pid });

  return {
    pid: result.pid,
    state: result.state,
    action: result.action,
  };
}

export async function stopSessionOutput(pid: number): Promise<TurnControlResult> {
  const result = await invoke<{
    pid: number;
    state: string;
    action: string;
  }>("stop_session_output", { pid });

  return {
    pid: result.pid,
    state: result.state,
    action: result.action,
  };
}

export async function deleteSession(sessionId: string): Promise<void> {
  await invoke("delete_session", { sessionId });
}
