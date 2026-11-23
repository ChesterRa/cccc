import { DirectInvitationCard } from "./DirectInvitationCard";
import { readDirectInvitation } from "./directConnectionModel";

export function DirectInvitationShare({
  text,
  framed = true,
  onDelete,
}: {
  text: string;
  framed?: boolean;
  onDelete?: () => void;
}) {
  const preview = readDirectInvitation(text);
  if (!preview) return null;
  return (
    <DirectInvitationCard
      invitation={{ ...preview, reusable: !!preview.reusable }}
      cachedText={text}
      framed={framed}
      onRead={() => Promise.resolve(text)}
      onDelete={onDelete}
    />
  );
}
