export function SidebarMobileOverlay({ open, onClose }: { open: boolean; onClose: () => void }) {
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-30 glass-overlay animate-fade-in md:hidden"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
      aria-hidden="true"
    />
  );
}
