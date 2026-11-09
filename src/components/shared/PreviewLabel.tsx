/**
 * The "Unencrypted preview" label (D-F08-7, AC-F10-10).
 *
 * Development builds may transfer without encryption until encrypted pairing
 * (F08) lands; release builds cannot transfer at all. This label is always
 * shown in a development build and cannot be dismissed, so nobody mistakes a
 * preview for the finished, secure product.
 */
export function PreviewLabel() {
  if (!import.meta.env.DEV) return null;

  return (
    <p
      role="status"
      className="border-b border-border bg-muted px-4 py-1.5 text-xs text-muted-foreground"
    >
      <strong className="font-semibold text-foreground">Unencrypted preview.</strong> Files are
      sent without encryption between computers on this link. Use only with computers you own.
    </p>
  );
}
