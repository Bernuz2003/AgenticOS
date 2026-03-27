import type { HumanInputRequest, TimelineItem } from "../../../lib/api";
import { buildTimelineSignature } from "../../../lib/timeline/mapping";

export function timelineSignature(items: TimelineItem[]): string {
  return buildTimelineSignature(items);
}

export function composerPlaceholder(
  humanRequest: HumanInputRequest | null,
  canSend: boolean,
): string {
  if (humanRequest) {
    return humanRequest.allowFreeText
      ? humanRequest.placeholder ?? "Inserisci la risposta umana richiesta..."
      : "Questo step richiede una scelta esplicita dalle opzioni sopra.";
  }

  return canSend
    ? "Invia un messaggio o un prompt all'agente..."
    : "Il composer si abilita quando il processo entra in WaitingForInput o WaitingForHumanInput...";
}
