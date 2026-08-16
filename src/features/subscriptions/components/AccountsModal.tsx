import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { AuthWorkspace } from '../../auth/AuthWorkspace';

/**
 * Site accounts manager in a modal — wraps the existing auth workspace
 * (per-site browser login and OAuth) so credentials are
 * reachable without leaving the subscriptions screen.
 */
export function AccountsModal({
  open,
  focusSiteId,
  onClose,
}: {
  open: boolean;
  focusSiteId: string | null;
  onClose: () => void;
}) {
  return (
    <GlassModal open={open} onClose={onClose} title="Site accounts" size="lg" flush>
      <AuthWorkspace externalSelectedSiteId={focusSiteId} />
    </GlassModal>
  );
}
