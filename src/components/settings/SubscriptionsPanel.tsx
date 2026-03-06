import { useState, useEffect, useRef } from 'react';
import { useDisclosure } from '@mantine/hooks';
import {
  Text,
  TextInput,
  NumberInput,
  Select,
  Loader,
  Modal,
  Collapse,
} from '@mantine/core';
import { glassModalStyles } from '../../styles/glassModal';
import {
  IconDownload,
  IconCalendarTime,
  IconPlayerPlay,
  IconPlayerPause,
  IconCheck,
  IconTrash,
  IconChevronDown,
  IconChevronRight,
  IconPlus,
  IconClock,
  IconPlayerStop,
} from '@tabler/icons-react';
import { notifySuccess, notifyError, notifyWarning } from '../../lib/notify';
import { SubscriptionController } from '../../controllers/subscriptionController';
import { registerUndoAction } from '../../controllers/undoRedoController';
import { useTaskRuntimeStore } from '../../stores/taskRuntimeStore';
import { TextButton } from '../ui/TextButton';
import { EmptyState } from '../ui/EmptyState';
import { SettingsBlock, SettingsButtonRow, SettingsInputGroup } from './ui';
import styles from '../Settings.module.css';

interface SiteInfo {
  id: string;
  name: string;
  url_template: string;
  example_query: string;
}

interface SubscriptionQueryInfo {
  id: string;
  queryText: string;
  displayName: string | null;
  paused: boolean;
  lastCheckTime: string | null;
  filesFound: number;
  completedInitialRun: boolean;
}

interface SubscriptionInfo {
  id: string;
  name: string;
  siteId: string;
  paused: boolean;
  initialFileLimit: number;
  periodicFileLimit: number;
  createdAt: string;
  queries: SubscriptionQueryInfo[];
}

export function SubscriptionsPanel() {
  const {
    ensureInitialized,
    runningSubs,
    runningQueries,
    lastSubscriptionFinished,
    subscriptionEventSeq,
  } = useTaskRuntimeStore((s) => ({
    ensureInitialized: s.ensureInitialized,
    runningSubs: s.runningSubscriptionIds,
    runningQueries: s.runningQueryIds,
    lastSubscriptionFinished: s.lastSubscriptionFinished,
    subscriptionEventSeq: s.subscriptionEventSeq,
  }));

  const [subscriptions, setSubscriptions] = useState<SubscriptionInfo[]>([]);
  const [sites, setSites] = useState<SiteInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [createModalOpen, { open: openCreateModal, close: closeCreateModal }] = useDisclosure(false);
  const [creating, setCreating] = useState(false);
  const [expandedSubs, setExpandedSubs] = useState<Set<string>>(new Set());
  const [addQuerySubId, setAddQuerySubId] = useState<string | null>(null);
  const [newQueryText, setNewQueryText] = useState('');
  const lastFinishedKeyRef = useRef<string | null>(null);

  const [newSubscription, setNewSubscription] = useState({
    name: '',
    site_id: '',
    queries: [''],
    initial_file_limit: 100,
    periodic_file_limit: 50,
  });

  useEffect(() => {
    void ensureInitialized();
    loadData();
  }, [ensureInitialized]);

  useEffect(() => {
    if (!subscriptionEventSeq) return;
    loadData();
  }, [subscriptionEventSeq]);

  useEffect(() => {
    if (!lastSubscriptionFinished) return;
    const key = [
      lastSubscriptionFinished.subscription_id,
      lastSubscriptionFinished.query_id ?? '',
      lastSubscriptionFinished.status,
      lastSubscriptionFinished.files_downloaded,
      lastSubscriptionFinished.files_skipped,
      lastSubscriptionFinished.error ?? '',
      lastSubscriptionFinished.failure_kind ?? '',
    ].join(':');
    if (lastFinishedKeyRef.current === key) return;
    lastFinishedKeyRef.current = key;

    if (lastSubscriptionFinished.status === 'succeeded') {
      notifySuccess(
        `Downloaded ${lastSubscriptionFinished.files_downloaded}, skipped ${lastSubscriptionFinished.files_skipped}`,
        'Sync Complete',
      );
    } else if (lastSubscriptionFinished.status === 'cancelled') {
      if (lastSubscriptionFinished.failure_kind === 'inbox_full') {
        notifyWarning('Paused — inbox is full (1000). Review inbox items to continue.', 'Sync Paused');
      } else {
        notifyWarning(`Cancelled — ${lastSubscriptionFinished.files_downloaded} downloaded`, 'Sync Stopped');
      }
    } else if (lastSubscriptionFinished.status === 'failed') {
      notifyError(lastSubscriptionFinished.error || `${lastSubscriptionFinished.errors_count} error(s)`, 'Sync Failed');
    }
  }, [lastSubscriptionFinished]);

  const loadData = async () => {
    try {
      setLoading(true);
      const [subsData, sitesData] = await Promise.all([
        SubscriptionController.getSubscriptions<SubscriptionInfo>(),
        SubscriptionController.getSites<SiteInfo>(),
      ]);
      setSubscriptions(subsData);
      setSites(sitesData);
    } catch (err) {
      console.error('Failed to load subscriptions:', err);
      notifyError('Failed to load subscription data');
    } finally {
      setLoading(false);
    }
  };

  const toggleExpanded = (id: string) => {
    setExpandedSubs(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleCreateSubscription = async () => {
    if (!newSubscription.name.trim() || !newSubscription.site_id || newSubscription.queries.filter(q => q.trim()).length === 0) {
      notifyError('Please fill in name, site, and at least one query', 'Missing Fields');
      return;
    }

    try {
      setCreating(true);
      const args = {
        name: newSubscription.name.trim(),
        siteId: newSubscription.site_id,
        queries: newSubscription.queries.filter(q => q.trim()),
        initialFileLimit: newSubscription.initial_file_limit > 0 ? newSubscription.initial_file_limit : null,
        periodicFileLimit: newSubscription.periodic_file_limit > 0 ? newSubscription.periodic_file_limit : null,
      };
      let created = await SubscriptionController.createSubscription(args);
      registerUndoAction({
        label: `Create subscription "${args.name}"`,
        undo: async () => {
          if (created?.id) {
            await SubscriptionController.deleteSubscription({ id: created.id, deleteFiles: false });
          }
          await loadData();
        },
        redo: async () => {
          created = await SubscriptionController.createSubscription(args);
          await loadData();
        },
      });
      notifySuccess(`"${newSubscription.name}" created`, 'Created');
      setNewSubscription({ name: '', site_id: '', queries: [''], initial_file_limit: 100, periodic_file_limit: 50 });
      closeCreateModal();
      await loadData();
    } catch (err) {
      notifyError(`Failed to create subscription: ${err}`);
    } finally {
      setCreating(false);
    }
  };

  const togglePause = async (sub: SubscriptionInfo) => {
    try {
      const nextPaused = !sub.paused;
      await SubscriptionController.pauseSubscription({ id: sub.id, paused: nextPaused });
      registerUndoAction({
        label: `${nextPaused ? 'Pause' : 'Resume'} subscription "${sub.name}"`,
        undo: async () => {
          await SubscriptionController.pauseSubscription({ id: sub.id, paused: sub.paused });
          await loadData();
        },
        redo: async () => {
          await SubscriptionController.pauseSubscription({ id: sub.id, paused: nextPaused });
          await loadData();
        },
      });
      await loadData();
    } catch (err) {
      notifyError(err);
    }
  };

  const deleteSub = async (sub: SubscriptionInfo) => {
    try {
      // Keep delete undoable: do not couple settings deletion to destructive file deletion.
      await SubscriptionController.deleteSubscription({ id: sub.id, deleteFiles: false });
      let recreated: { id: string } | null = null;
      const snapshot = {
        name: sub.name,
        siteId: sub.siteId,
        queries: sub.queries.map((q) => q.queryText),
        initialFileLimit: sub.initialFileLimit,
        periodicFileLimit: sub.periodicFileLimit,
        paused: sub.paused,
      };
      registerUndoAction({
        label: `Delete subscription "${sub.name}"`,
        undo: async () => {
          recreated = await SubscriptionController.createSubscription(snapshot);
          if (snapshot.paused && recreated?.id) {
            await SubscriptionController.pauseSubscription({ id: recreated.id, paused: true });
          }
          await loadData();
        },
        redo: async () => {
          const id = recreated?.id ?? sub.id;
          await SubscriptionController.deleteSubscription({ id, deleteFiles: false });
          await loadData();
        },
      });
      notifySuccess(`"${sub.name}" deleted`, 'Deleted');
      await loadData();
    } catch (err) {
      notifyError(err);
    }
  };

  const runSub = async (sub: SubscriptionInfo) => {
    try {
      await SubscriptionController.runSubscription({ id: sub.id });
    } catch (err) {
      notifyError(err, 'Sync Failed');
    }
  };

  const stopSub = async (sub: SubscriptionInfo) => {
    try {
      await SubscriptionController.stopSubscription({ id: sub.id });
    } catch (err) {
      notifyError(`Failed to stop: ${err}`);
    }
  };

  const runQuery = async (sub: SubscriptionInfo, query: SubscriptionQueryInfo) => {
    try {
      await SubscriptionController.runSubscriptionQuery({
        queryId: query.id,
        subscriptionId: sub.id,
      });
    } catch (err) {
      notifyError(err, 'Sync Failed');
    }
  };

  const toggleQueryPause = async (query: SubscriptionQueryInfo) => {
    try {
      const nextPaused = !query.paused;
      await SubscriptionController.pauseSubscriptionQuery({ id: query.id, paused: nextPaused });
      registerUndoAction({
        label: `${nextPaused ? 'Pause' : 'Resume'} query`,
        undo: async () => {
          await SubscriptionController.pauseSubscriptionQuery({ id: query.id, paused: query.paused });
          await loadData();
        },
        redo: async () => {
          await SubscriptionController.pauseSubscriptionQuery({ id: query.id, paused: nextPaused });
          await loadData();
        },
      });
      await loadData();
    } catch (err) {
      notifyError(err);
    }
  };

  const deleteQuery = async (sub: SubscriptionInfo, query: SubscriptionQueryInfo) => {
    try {
      await SubscriptionController.deleteSubscriptionQuery({ id: query.id });
      let recreated: { id: string; paused: boolean } | null = null;
      const snapshot = { text: query.queryText, paused: query.paused };
      registerUndoAction({
        label: 'Delete query',
        undo: async () => {
          recreated = await SubscriptionController.addSubscriptionQuery({
            subscriptionId: sub.id,
            queryText: snapshot.text,
          });
          if (snapshot.paused && recreated?.id) {
            await SubscriptionController.pauseSubscriptionQuery({ id: recreated.id, paused: true });
          }
          await loadData();
        },
        redo: async () => {
          const id = recreated?.id ?? query.id;
          await SubscriptionController.deleteSubscriptionQuery({ id });
          await loadData();
        },
      });
      await loadData();
    } catch (err) {
      notifyError(err);
    }
  };

  const handleAddQuery = async () => {
    if (!addQuerySubId || !newQueryText.trim()) return;
    try {
      const queryText = newQueryText.trim();
      const subId = addQuerySubId;
      let added = await SubscriptionController.addSubscriptionQuery({
        subscriptionId: addQuerySubId,
        queryText,
      });
      registerUndoAction({
        label: 'Add query',
        undo: async () => {
          if (added?.id) {
            await SubscriptionController.deleteSubscriptionQuery({ id: added.id });
          }
          await loadData();
        },
        redo: async () => {
          added = await SubscriptionController.addSubscriptionQuery({
            subscriptionId: subId,
            queryText,
          });
          await loadData();
        },
      });
      setNewQueryText('');
      setAddQuerySubId(null);
      await loadData();
    } catch (err) {
      notifyError(err);
    }
  };

  const addQueryField = () => {
    setNewSubscription(prev => ({ ...prev, queries: [...prev.queries, ''] }));
  };

  const removeQueryField = (index: number) => {
    setNewSubscription(prev => ({ ...prev, queries: prev.queries.filter((_, i) => i !== index) }));
  };

  const updateQuery = (index: number, value: string) => {
    setNewSubscription(prev => ({ ...prev, queries: prev.queries.map((q, i) => i === index ? value : q) }));
  };

  const getSiteName = (siteId: string) => sites.find(s => s.id === siteId)?.name ?? siteId;

  const getSelectedSite = () => sites.find(s => s.id === newSubscription.site_id);

  const formatTime = (iso: string | null) => {
    if (!iso) return 'Never';
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  };

  if (loading) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}>
        <Loader size="sm" />
      </div>
    );
  }

  return (
    <>
      {/* Actions */}
      <SettingsBlock title="Manage" description="Automated gallery downloading via gallery-dl.">
        <SettingsButtonRow>
          <TextButton onClick={() => openCreateModal()}>
            <IconDownload size={14} />
            New Subscription
          </TextButton>
        </SettingsButtonRow>
      </SettingsBlock>

      {/* Subscription list */}
      <SettingsBlock title="Subscriptions">
        {subscriptions.length === 0 ? (
          <EmptyState compact icon={IconCalendarTime} description="No subscriptions yet." />
        ) : (
          subscriptions.map((sub, i) => {
            const isExpanded = expandedSubs.has(sub.id);
            const isRunning = runningSubs.has(sub.id);
            return (
              <div key={sub.id}>
                {i > 0 && <div className={styles.blockSeparator} />}
                {/* Header row */}
                <div
                  className={styles.labelItem}
                  style={{ cursor: 'pointer', minHeight: 32 }}
                  onClick={() => toggleExpanded(sub.id)}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, flex: 1, minWidth: 0 }}>
                    {isExpanded ? <IconChevronDown size={12} /> : <IconChevronRight size={12} />}
                    <label style={{ cursor: 'pointer', paddingTop: 0 }}>{sub.name}</label>
                    <Text size="xs" c={sub.paused ? 'orange' : 'teal'}>
                      {sub.paused ? 'paused' : 'active'}
                    </Text>
                    <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
                      {getSiteName(sub.siteId)} · {sub.queries.length} quer{sub.queries.length === 1 ? 'y' : 'ies'}
                    </Text>
                  </div>
                  <div className={styles.right} onClick={(e) => e.stopPropagation()} style={{ gap: 4 }}>
                    <TextButton compact onClick={() => togglePause(sub)}>
                      {sub.paused ? <IconPlayerPlay size={12} /> : <IconPlayerPause size={12} />}
                    </TextButton>
                    {isRunning ? (
                      <TextButton compact danger onClick={() => stopSub(sub)}>
                        <IconPlayerStop size={12} />
                      </TextButton>
                    ) : (
                      <TextButton compact onClick={() => runSub(sub)}>
                        <IconDownload size={12} />
                      </TextButton>
                    )}
                    <TextButton compact danger onClick={() => deleteSub(sub)}>
                      <IconTrash size={12} />
                    </TextButton>
                  </div>
                </div>

                {/* Expanded queries */}
                <Collapse in={isExpanded}>
                  <div style={{ paddingLeft: 20, marginTop: 4 }}>
                    {sub.queries.map((query) => {
                      const qRunning = runningQueries.has(query.id);
                      return (
                        <div key={query.id} className={styles.labelItem} style={{ minHeight: 28 }}>
                          <div style={{ flex: 1, minWidth: 0 }}>
                            <Text size="xs" fw={500}>{query.displayName || query.queryText}</Text>
                            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                              <IconClock size={10} style={{ opacity: 0.5 }} />
                              <Text size="xs" c="dimmed">{formatTime(query.lastCheckTime)}</Text>
                              {query.filesFound > 0 && (
                                <Text size="xs" c="dimmed">· {query.filesFound} files</Text>
                              )}
                              {query.paused && (
                                <Text size="xs" c="orange">paused</Text>
                              )}
                            </div>
                          </div>
                          <div className={styles.right} style={{ gap: 4 }}>
                            <TextButton compact onClick={() => toggleQueryPause(query)}>
                              {query.paused ? <IconPlayerPlay size={10} /> : <IconPlayerPause size={10} />}
                            </TextButton>
                            <TextButton compact onClick={() => runQuery(sub, query)} disabled={qRunning}>
                              {qRunning ? <Loader size={10} /> : <IconDownload size={10} />}
                            </TextButton>
                            <TextButton compact danger onClick={() => deleteQuery(sub, query)}>
                              <IconTrash size={10} />
                            </TextButton>
                          </div>
                        </div>
                      );
                    })}

                    {/* Add query inline */}
                    {addQuerySubId === sub.id ? (
                      <SettingsInputGroup>
                        <TextInput
                          size="xs"
                          placeholder="e.g., blue_eyes blonde_hair"
                          value={newQueryText}
                          onChange={(e) => setNewQueryText(e.currentTarget.value)}
                          style={{ flex: 1 }}
                          onKeyDown={(e) => e.key === 'Enter' && handleAddQuery()}
                        />
                        <TextButton compact onClick={handleAddQuery}>
                          <IconCheck size={12} />
                        </TextButton>
                      </SettingsInputGroup>
                    ) : (
                      <div style={{ marginTop: 4, marginBottom: 4 }}>
                        <TextButton
                          compact
                          onClick={() => { setAddQuerySubId(sub.id); setNewQueryText(''); }}
                        >
                          <IconPlus size={12} />
                          Add Query
                        </TextButton>
                      </div>
                    )}
                  </div>
                </Collapse>
              </div>
            );
          })
        )}
      </SettingsBlock>

      {/* Create Subscription Modal */}
      <Modal opened={createModalOpen} onClose={closeCreateModal} title="New Subscription" size="md" styles={glassModalStyles}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <TextInput
            label="Name"
            placeholder="e.g., Artist Name - Tag Collection"
            value={newSubscription.name}
            onChange={(e) => setNewSubscription(prev => ({ ...prev, name: e.currentTarget.value }))}
            required
          />

          <Select
            label="Site"
            placeholder="Select a site"
            value={newSubscription.site_id}
            onChange={(value) => setNewSubscription(prev => ({ ...prev, site_id: value || '' }))}
            data={sites.map(site => ({ value: site.id, label: site.name }))}
            required
            searchable
          />

          <div>
            <Text size="sm" fw={500} mb="xs">Queries</Text>
            {getSelectedSite() && (
              <Text size="xs" c="dimmed" mb="xs">e.g., {getSelectedSite()!.example_query}</Text>
            )}
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {newSubscription.queries.map((query, index) => (
                <SettingsInputGroup key={index}>
                  <TextInput
                    placeholder={getSelectedSite()?.example_query ?? 'e.g., blue_eyes blonde_hair'}
                    value={query}
                    onChange={(e) => updateQuery(index, e.currentTarget.value)}
                    style={{ flex: 1 }}
                  />
                  {newSubscription.queries.length > 1 && (
                    <TextButton compact danger onClick={() => removeQueryField(index)}>
                      <IconTrash size={14} />
                    </TextButton>
                  )}
                </SettingsInputGroup>
              ))}
              <TextButton compact onClick={addQueryField}>
                <IconPlus size={14} />
                Add Query
              </TextButton>
            </div>
          </div>

          <div style={{ display: 'flex', gap: 12 }}>
            <NumberInput
              label="Initial Limit"
              description="Max files on first run"
              value={newSubscription.initial_file_limit}
              onChange={(value) => setNewSubscription(prev => ({ ...prev, initial_file_limit: Number(value) || 0 }))}
              min={0} max={5000}
              style={{ flex: 1 }}
            />
            <NumberInput
              label="Periodic Limit"
              description="Max files per check"
              value={newSubscription.periodic_file_limit}
              onChange={(value) => setNewSubscription(prev => ({ ...prev, periodic_file_limit: Number(value) || 0 }))}
              min={0} max={5000}
              style={{ flex: 1 }}
            />
          </div>

          <SettingsButtonRow>
            <TextButton onClick={() => closeCreateModal()}>Cancel</TextButton>
            <TextButton onClick={handleCreateSubscription} disabled={creating}>
              <IconCheck size={14} />
              Create
            </TextButton>
          </SettingsButtonRow>
        </div>
      </Modal>
    </>
  );
}
