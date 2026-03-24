import { useMemo } from 'react';
import { IconCheck, IconLoader2, IconX } from '@tabler/icons-react';

import { useSubscriptionProgressStore, type RuntimeSubscriptionProgress } from '../../../state-legacy/taskStore';
import st from './SidebarJobStatus.module.css';

interface GroupSummary {
  groupName: string;
  totalFiles: number;
  totalPosts: number;
  subs: RuntimeSubscriptionProgress[];
}

export function SidebarJobStatus() {
  const subscriptionProgressById = useSubscriptionProgressStore((s) => s.subscriptionProgressById);

  const groups = useMemo(() => {
    const subs = [...subscriptionProgressById.values()].sort((a, b) =>
      a.subscription_id.localeCompare(b.subscription_id),
    );
    // Group by group_name (or subscription_id if no group)
    const map = new Map<string, GroupSummary>();
    for (const sub of subs) {
      const key = sub.group_name ?? `__sub_${sub.subscription_id}`;
      let group = map.get(key);
      if (!group) {
        group = { groupName: sub.group_name ?? '', totalFiles: 0, totalPosts: 0, subs: [] };
        map.set(key, group);
      }
      group.totalFiles += sub.files_downloaded;
      group.totalPosts += sub.posts_processed;
      group.subs.push(sub);
    }
    return [...map.values()];
  }, [subscriptionProgressById]);

  if (groups.length === 0) return null;

  return (
    <div className={st.root}>
      <div className={st.subList}>
        {groups.map((group) => (
          <GroupCard key={group.subs[0].subscription_id} group={group} />
        ))}
      </div>
    </div>
  );
}

function GroupCard({ group }: { group: GroupSummary }) {
  // If group has multiple subs, show group header with totals
  const showGroupHeader = group.groupName && group.subs.length > 1;

  return (
    <>
      {showGroupHeader && (
        <div className={st.groupHeader}>
          <span className={st.groupName}>{group.groupName}</span>
          <span className={st.jobCounters}>
            Files:{group.totalFiles} Posts:{group.totalPosts}
          </span>
        </div>
      )}
      {group.subs.map((sub) => (
        <JobCard key={sub.subscription_id} sub={sub} showGroup={!showGroupHeader} />
      ))}
    </>
  );
}

function JobCard({ sub, showGroup }: { sub: RuntimeSubscriptionProgress; showGroup: boolean }) {
  const isRunning = sub.status === 'running';
  const isFailed = sub.finished_status === 'failed' || sub.finished_status === 'cancelled';
  const isSuccess = sub.finished_status === 'succeeded';

  const nameParts: string[] = [];
  if (showGroup && sub.group_name) nameParts.push(sub.group_name);
  nameParts.push(sub.subscription_name || `Subscription ${sub.subscription_id}`);
  const displayName = nameParts.join(' › ');

  const hasError = !!sub.last_error && (isFailed || sub.phase === 'finished');
  const phaseText = hasError ? sub.last_error! : sub.status_text;

  return (
    <div className={st.jobCard}>
      <div className={st.jobNameRow}>
        <span className={st.jobName}>{displayName}</span>
        <span className={st.jobCounters}>
          Files:{sub.files_downloaded} Posts:{sub.posts_processed}
        </span>
      </div>
      <div className={st.jobPhaseRow}>
        <span className={st.jobStatusIcon}>
          {isRunning ? (
            <IconLoader2 size={10} className={st.spinner} />
          ) : isSuccess ? (
            <IconCheck size={10} />
          ) : (
            <IconX size={10} />
          )}
        </span>
        <span className={`${st.jobPhase} ${hasError ? st.jobPhaseError : ''}`}>
          {phaseText}
        </span>
      </div>
    </div>
  );
}
