import { TimelinePane, type TimelinePaneProps } from "../timeline-pane";

export function ConversationSurface(props: TimelinePaneProps) {
  return (
    <section className="panel-surface flex min-h-0 flex-1 overflow-hidden">
      <TimelinePane {...props} />
    </section>
  );
}
