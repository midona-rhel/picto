import { useAtomValue, useSetAtom } from 'jotai';
import { libraryCoverModalAtom } from '../../state/modals';
import { showErrorNotification } from '../../shared/lib/notifications';
import { MediaCoverDialog } from '../subscriptions/components/SubscriptionCoverDialog';
import { loadLibraryCoverCandidates, saveLibraryCover } from './libraryAppearance';

export function LibraryCoverDialogHost() {
  const state = useAtomValue(libraryCoverModalAtom);
  const setState = useSetAtom(libraryCoverModalAtom);

  return (
    <MediaCoverDialog<string>
      target={state.open ? { id: state.path, name: state.name } : null}
      busy={false}
      initialCandidate={state.initialCandidate}
      instructions="Select a media item from this library, then adjust its position and zoom."
      emptyText="This library has no media available for a cover."
      onLoad={(path, cursor) => loadLibraryCoverCandidates(path, cursor ?? null)}
      onSave={async (path, candidate, crop) => {
        try {
          await saveLibraryCover(path, candidate, crop);
          return true;
        } catch (reason) {
          showErrorNotification({
            title: 'Could not set library cover',
            message: reason instanceof Error ? reason.message : String(reason),
          });
          return false;
        }
      }}
      onClose={() => setState({ open: false, path: '', name: '', initialCandidate: null })}
    />
  );
}
