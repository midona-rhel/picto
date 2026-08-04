import { useEffect, useMemo, useState } from 'react';
import { IconChevronRight, IconShieldLock } from '@tabler/icons-react';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../../shared/ui/ToggleSwitch/ToggleSwitch';
import { ActionButton } from './ActionButton';
import { TagAutocompleteInput } from './TagAutocompleteInput';
import type {
  SubscriptionSiteInfo,
} from '../../../shared/types/subscriptions';
import styles from './NewSubscriptionDialog.module.css';

export interface CreateSubscriptionInput {
  name: string;
  siteId: string;
  queryText: string;
  initialPostLimit: number;
  periodicPostLimit: number;
  autoCollections: boolean;
  runNow: boolean;
}

/**
 * Compact single-form dialog for creating a subscription and its first query.
 * Sync limits and collection behaviour live under Advanced.
 */
export function NewSubscriptionDialog({
  open,
  sites,
  credentialSiteCategories,
  initialSiteId,
  busy,
  onOpenAccounts,
  onCreate,
  onClose,
}: {
  open: boolean;
  sites: SubscriptionSiteInfo[];
  credentialSiteCategories: Set<string>;
  initialSiteId: string | null;
  busy: boolean;
  onOpenAccounts: (siteId: string) => void;
  onCreate: (result: CreateSubscriptionInput) => void;
  onClose: () => void;
}) {
  const [siteId, setSiteId] = useState(initialSiteId ?? sites[0]?.id ?? '');
  const [queryText, setQueryText] = useState('');
  const [name, setName] = useState('');
  const [nameTouched, setNameTouched] = useState(false);
  const [initialLimit, setInitialLimit] = useState('100');
  const [periodicLimit, setPeriodicLimit] = useState('50');
  const [autoCollections, setAutoCollections] = useState(true);
  const [showAdvanced, setShowAdvanced] = useState(false);

  // Sites load async — settle the default selection when the dialog opens.
  useEffect(() => {
    if (open && !sites.some((entry) => entry.id === siteId)) {
      setSiteId(initialSiteId ?? sites[0]?.id ?? '');
    }
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const site = sites.find((entry) => entry.id === siteId) ?? null;
  const hasCredential = site != null
    && credentialSiteCategories.has(site.credential_owner_site_id);
  const authMissing = site != null && site.auth_supported && !hasCredential;
  const authBlocking = authMissing && site.auth_strictly_required;

  const derivedName =
    site && queryText.trim() ? `${site.name} — ${queryText.trim()}` : '';
  const effectiveName = nameTouched ? name : derivedName;

  const siteOptions = useMemo(
    () => sites.map((entry) => ({ value: entry.id, label: entry.name })),
    [sites],
  );

  const canCreate =
    siteId !== '' && queryText.trim() !== '' && !authBlocking && !busy;

  const reset = () => {
    setSiteId(initialSiteId ?? sites[0]?.id ?? '');
    setQueryText('');
    setName('');
    setNameTouched(false);
    setInitialLimit('100');
    setPeriodicLimit('50');
    setAutoCollections(true);
    setShowAdvanced(false);
  };

  const close = () => {
    reset();
    onClose();
  };

  const submit = (runNow: boolean) => {
    const parsedInitial = Number.parseInt(initialLimit, 10);
    const parsedPeriodic = Number.parseInt(periodicLimit, 10);
    onCreate({
      name: effectiveName.trim() || `${site?.name ?? 'Subscription'} — ${queryText.trim()}`,
      siteId,
      queryText: queryText.trim(),
      initialPostLimit: Number.isFinite(parsedInitial) && parsedInitial > 0 ? parsedInitial : 100,
      periodicPostLimit: Number.isFinite(parsedPeriodic) && parsedPeriodic > 0 ? parsedPeriodic : 50,
      autoCollections,
      runNow,
    });
    reset();
  };

  return (
    <GlassModal
      open={open}
      onClose={close}
      title="New subscription"
      size="sm"
      footer={
        <>
          <ActionButton variant="ghost" onClick={close}>Cancel</ActionButton>
          <ActionButton variant="secondary" disabled={!canCreate} onClick={() => submit(false)}>
            Create
          </ActionButton>
          <ActionButton variant="primary" disabled={!canCreate} onClick={() => submit(true)}>
            Create &amp; run now
          </ActionButton>
        </>
      }
    >
      <div className={styles.form}>
        <div className={styles.row}>
          <span className={styles.rowLabel}>Site</span>
          <div className={styles.rowControl}>
            <CmSelect value={siteId} options={siteOptions} onChange={setSiteId} width={220} />
            {site && <span className={styles.siteDomain}>{site.domain}</span>}
          </div>
        </div>

        {authMissing && (
          <div className={styles.row}>
            <span className={styles.rowLabel} />
            <div className={`${styles.authNote} ${authBlocking ? styles.authNoteBlocking : ''}`.trim()}>
              <IconShieldLock size={13} />
              <span>
                {authBlocking
                  ? `${site.name} requires a login.`
                  : 'Works better with a login — some posts may be hidden without one.'}
              </span>
              <button
                type="button"
                className={styles.authLink}
                onClick={() => onOpenAccounts(site.credential_owner_site_id)}
              >
                Connect account…
              </button>
            </div>
          </div>
        )}

        <div className={styles.row}>
          <span className={styles.rowLabel}>
            {site?.supports_query === false ? 'Account' : 'Query'}
          </span>
          <div className={styles.rowControlColumn}>
            <TagAutocompleteInput
              siteId={siteId}
              value={queryText}
              onChange={setQueryText}
              placeholder={site ? `e.g. ${site.example_query}` : 'query'}
              autoFocus
            />
            {site && queryText.trim() ? (
              <span className={styles.urlPreview}>
                {site.url_template.replace('{query}', queryText.trim())}
              </span>
            ) : (
              <span className={styles.helper}>
                New matching posts are downloaded into your library automatically.
              </span>
            )}
          </div>
        </div>

        <div className={styles.row}>
          <span className={styles.rowLabel}>Name</span>
          <div className={styles.rowControl}>
            <input
              className={styles.textInput}
              value={effectiveName}
              placeholder={site ? `${site.name} — …` : 'Name'}
              onChange={(e) => {
                setNameTouched(true);
                setName(e.target.value);
              }}
            />
          </div>
        </div>

        <button
          type="button"
          className={styles.advancedToggle}
          aria-expanded={showAdvanced}
          onClick={() => setShowAdvanced((v) => !v)}
        >
          <IconChevronRight
            size={12}
            className={`${styles.advancedChevron} ${showAdvanced ? styles.advancedChevronOpen : ''}`.trim()}
          />
          Advanced
        </button>

        {showAdvanced && (
          <div className={styles.advancedBody}>
            <div className={styles.row}>
              <span className={styles.rowLabel}>First sync</span>
              <div className={styles.rowControl}>
                <input
                  className={`${styles.textInput} ${styles.numInput}`}
                  value={initialLimit}
                  inputMode="numeric"
                  onChange={(e) => setInitialLimit(e.target.value)}
                />
                <span className={styles.helper}>posts on the first run</span>
              </div>
            </div>
            <div className={styles.row}>
              <span className={styles.rowLabel}>Later checks</span>
              <div className={styles.rowControl}>
                <input
                  className={`${styles.textInput} ${styles.numInput}`}
                  value={periodicLimit}
                  inputMode="numeric"
                  onChange={(e) => setPeriodicLimit(e.target.value)}
                />
                <span className={styles.helper}>new posts per check</span>
              </div>
            </div>
            <div className={styles.row}>
              <span className={styles.rowLabel}>Collections</span>
              <div className={styles.rowControl}>
                <ToggleSwitch on={autoCollections} onChange={() => setAutoCollections((v) => !v)} />
                <span className={styles.helper}>group multi-image posts into one collection</span>
              </div>
            </div>
          </div>
        )}
      </div>
    </GlassModal>
  );
}
