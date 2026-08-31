import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { AuthWorkspace } from '../../auth/AuthWorkspace';
import { t } from '../../../i18n';

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
    <GlassModal open={open} onClose={onClose} title={t("Site accounts")} size="xl" flush>
      <AuthWorkspace externalSelectedSiteId={focusSiteId} />
    </GlassModal>
  );
}
