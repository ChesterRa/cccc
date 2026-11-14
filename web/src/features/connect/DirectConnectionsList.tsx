import { DirectConnectionRow } from "./DirectConnectionRow";
import type { DirectRelation } from "./directConnectionModel";

export function DirectConnectionsList({
  relations,
  invitations,
  busy,
  onAction,
}: {
  relations: DirectRelation[];
  invitations: Record<string, string>;
  busy: boolean;
  onAction: (action: string, id: string) => Promise<boolean>;
}) {
  return (
    <ul className="grid min-w-0 grid-cols-[repeat(auto-fit,minmax(min(100%,24rem),1fr))] gap-3">
      {relations.map((relation) => (
        <DirectConnectionRow
          key={relation.id}
          relation={relation}
          invitation={invitations[relation.id]}
          busy={busy}
          onAction={onAction}
        />
      ))}
    </ul>
  );
}
