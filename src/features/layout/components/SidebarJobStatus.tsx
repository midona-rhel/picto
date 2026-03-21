import { useMemo } from 'react';
import { IconDownload } from '@tabler/icons-react';

import { useSubscriptionProgressStore } from '../../subscriptions/subscriptionProgressStore';
import st from './SidebarJobStatus.module.css';

export function SidebarJobStatus() {
  const subscriptionProgressById = useSubscriptionProgressStore((s) => s.subscriptionProgressById);

  const subs = useMemo(() => {
    return [...subscriptionProgressById.values()].sort((a, b) =>
      a.subscription_id.localeCompare(b.subscription_id),
    );
  }, [subscriptionProgressById]);

  if (subs.length === 0) return null;

  return (
    <div className={st.root}>
      <div className={st.subList}>
        {subs.map((sub) => (
          <div key={sub.subscription_id} className={st.jobCard}>
            <div className={st.jobIcon}>
              <IconDownload size={14} />
            </div>
            <div className={st.jobNameRow}>
              <span className={st.jobName}>
                {(sub.query_name ?? '').trim() || sub.subscription_name}
              </span>
              <span className={st.jobPhase}>{sub.status_text}</span>
            </div>
            <div className={st.jobProgressRow}>
              <div className={st.jobProgress}>
                {sub.status === 'running' ? (
                  <div className={st.progressIndeterminate} />
                ) : (
                  <div className={st.progressFill} style={{ width: '100%' }} />
                )}
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
