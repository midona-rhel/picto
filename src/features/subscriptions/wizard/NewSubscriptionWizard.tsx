import { useMemo, useState } from 'react';
import { IconCheck, IconShieldLock } from '@tabler/icons-react';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../../shared/ui/ToggleSwitch/ToggleSwitch';
import { ActionButton } from '../components/ActionButton';
import { TagAutocompleteInput } from '../components/TagAutocompleteInput';
import type {
  SubscriptionGroupInfo,
  SubscriptionSiteInfo,
} from '../../../shared/types/subscriptions';
import styles from '../SubscriptionsScreen.module.css';

type StepKey = 'site' | 'query' | 'auth' | 'options';

export interface WizardResult {
  name: string;
  siteId: string;
  queryText: string;
  groupId: number | null;
  newGroupName: string | null;
  initialPostLimit: number;
  periodicPostLimit: number;
  autoCollections: boolean;
  runNow: boolean;
}

/**
 * Guided "Follow…" flow: pick a site, write a validated query, sort out
 * login when the site needs it, then name and file the subscription.
 */
export function NewSubscriptionWizard({
  open,
  sites,
  groups,
  credentialSiteCategories,
  initialSiteId,
  busy,
  onOpenAccounts,
  onCreate,
  onClose,
}: {
  open: boolean;
  sites: SubscriptionSiteInfo[];
  groups: SubscriptionGroupInfo[];
  credentialSiteCategories: Set<string>;
  initialSiteId: string | null;
  busy: boolean;
  onOpenAccounts: (siteId: string) => void;
  onCreate: (result: WizardResult) => void;
  onClose: () => void;
}) {
  const [siteId, setSiteId] = useState(initialSiteId ?? '');
  const [siteFilter, setSiteFilter] = useState('');
  const [queryText, setQueryText] = useState('');
  const [name, setName] = useState('');
  const [groupPick, setGroupPick] = useState('');
  const [newGroupName, setNewGroupName] = useState('');
  const [initialLimit, setInitialLimit] = useState('100');
  const [periodicLimit, setPeriodicLimit] = useState('50');
  const [autoCollections, setAutoCollections] = useState(true);
  const [step, setStep] = useState<StepKey>('site');

  const site = sites.find((entry) => entry.id === siteId) ?? null;
  const hasCredential = site != null && credentialSiteCategories.has(site.id);
  const needsAuthStep = site != null && site.auth_supported && !hasCredential;
  const authBlocking = needsAuthStep && site.auth_strictly_required;

  const steps = useMemo<StepKey[]>(() => {
    const list: StepKey[] = ['site', 'query'];
    if (needsAuthStep) list.push('auth');
    list.push('options');
    return list;
  }, [needsAuthStep]);

  const stepIndex = steps.indexOf(step);
  const filteredSites = sites.filter(
    (entry) =>
      entry.name.toLowerCase().includes(siteFilter.toLowerCase()) ||
      entry.domain.toLowerCase().includes(siteFilter.toLowerCase()),
  );

  const reset = () => {
    setSiteId(initialSiteId ?? '');
    setSiteFilter('');
    setQueryText('');
    setName('');
    setGroupPick('');
    setNewGroupName('');
    setInitialLimit('100');
    setPeriodicLimit('50');
    setAutoCollections(true);
    setStep('site');
  };

  const close = () => {
    reset();
    onClose();
  };

  const goNext = () => {
    if (step === 'query' && !name.trim()) {
      // Sensible default name: "Site — query"
      setName(site ? `${site.name} — ${queryText.trim()}` : queryText.trim());
    }
    const next = steps[stepIndex + 1];
    if (next) setStep(next);
  };

  const canAdvance =
    step === 'site'
      ? siteId !== ''
      : step === 'query'
        ? queryText.trim() !== ''
        : step === 'auth'
          ? !authBlocking || hasCredential
          : true;

  const submit = (runNow: boolean) => {
    const parsedInitial = Number.parseInt(initialLimit, 10);
    const parsedPeriodic = Number.parseInt(periodicLimit, 10);
    onCreate({
      name: name.trim() || `${site?.name ?? 'Subscription'} — ${queryText.trim()}`,
      siteId,
      queryText: queryText.trim(),
      groupId: groupPick && groupPick !== 'new' ? Number.parseInt(groupPick, 10) : null,
      newGroupName: groupPick === 'new' ? newGroupName.trim() || 'New group' : null,
      initialPostLimit: Number.isFinite(parsedInitial) && parsedInitial > 0 ? parsedInitial : 100,
      periodicPostLimit: Number.isFinite(parsedPeriodic) && parsedPeriodic > 0 ? parsedPeriodic : 50,
      autoCollections,
      runNow,
    });
    reset();
  };

  const stepLabel: Record<StepKey, string> = {
    site: 'Site',
    query: 'What to follow',
    auth: 'Login',
    options: 'Options',
  };

  return (
    <GlassModal
      open={open}
      onClose={close}
      title="Follow on a site"
      size="lg"
      footer={
        <>
          <ActionButton variant="ghost" onClick={close}>Cancel</ActionButton>
          {stepIndex > 0 && (
            <ActionButton variant="secondary" onClick={() => setStep(steps[stepIndex - 1])}>
              Back
            </ActionButton>
          )}
          {step !== 'options' ? (
            <ActionButton variant="primary" disabled={!canAdvance || busy} onClick={goNext}>
              Continue
            </ActionButton>
          ) : (
            <>
              <ActionButton variant="secondary" disabled={busy} onClick={() => submit(false)}>
                Create
              </ActionButton>
              <ActionButton variant="primary" disabled={busy} onClick={() => submit(true)}>
                Create & run now
              </ActionButton>
            </>
          )}
        </>
      }
    >
      <div className={styles.wizardSteps}>
        {steps.map((key, index) => (
          <span
            key={key}
            className={`${styles.wizardStep} ${
              index === stepIndex ? styles.wizardStepActive : index < stepIndex ? styles.wizardStepDone : ''
            }`.trim()}
          >
            {index < stepIndex && <IconCheck size={11} />}
            {index + 1}. {stepLabel[key]}
          </span>
        ))}
      </div>

      <div className={styles.wizardBody}>

      {step === 'site' && (
        <>
          <div className={styles.formField}>
            <GlassInput
              value={siteFilter}
              placeholder="Search sites…"
              autoFocus
              onChange={(e) => setSiteFilter(e.target.value)}
            />
          </div>
          <div className={styles.siteGrid}>
            {filteredSites.map((entry) => (
              <button
                key={entry.id}
                type="button"
                className={`${styles.siteCard} ${entry.id === siteId ? styles.siteCardSelected : ''}`.trim()}
                onClick={() => setSiteId(entry.id)}
                onDoubleClick={() => {
                  setSiteId(entry.id);
                  setStep('query');
                }}
              >
                <span className={styles.siteCardName}>{entry.name}</span>
                <span className={styles.siteCardDomain}>{entry.domain}</span>
                {entry.auth_strictly_required ? (
                  <span className={styles.siteCardAuth}>Login required</span>
                ) : entry.auth_supported ? (
                  <span className={styles.siteCardAuth}>Login available</span>
                ) : null}
              </button>
            ))}
          </div>
        </>
      )}

      {step === 'query' && site && (
        <>
          <div className={styles.formField}>
            <span className={styles.label}>
              {site.supports_query ? `Search tags on ${site.name}` : `Account on ${site.name}`}
            </span>
            <TagAutocompleteInput
              siteId={site.id}
              value={queryText}
              onChange={setQueryText}
              placeholder={`e.g. ${site.example_query}`}
              autoFocus
            />
            <span className={styles.helper}>
              {site.supports_query
                ? 'New posts matching these tags will be downloaded into your library automatically.'
                : 'New posts from this account will be downloaded into your library automatically.'}
            </span>
          </div>
          {queryText.trim() && (
            <span className={styles.urlPreview}>
              {site.url_template.replace('{query}', queryText.trim())}
            </span>
          )}
        </>
      )}

      {step === 'auth' && site && (
        <div className={styles.formField}>
          <span className={styles.label}>
            <IconShieldLock size={14} /> {site.name} {authBlocking ? 'requires a login' : 'works better with a login'}
          </span>
          <span className={styles.helper}>
            {authBlocking
              ? `${site.name} cannot be used without an account. Connect one to continue.`
              : `Without an account some posts may be hidden or limited. You can skip this and add one later.`}
          </span>
          <div className={styles.inlineActions}>
            <ActionButton variant="primary" onClick={() => onOpenAccounts(site.id)}>
              Connect {site.name} account…
            </ActionButton>
            {hasCredential && <span className={styles.helper}>✓ Account connected</span>}
          </div>
        </div>
      )}

      {step === 'options' && site && (
        <>
          <div className={styles.formField}>
            <span className={styles.label}>Name</span>
            <GlassInput value={name} autoFocus onChange={(e) => setName(e.target.value)} />
          </div>
          <div className={styles.formField}>
            <span className={styles.label}>Group</span>
            <CmSelect
              value={groupPick}
              options={[
                { value: '', label: 'No group' },
                ...groups.map((group) => ({ value: group.id, label: group.name })),
                { value: 'new', label: 'New group…' },
              ]}
              onChange={setGroupPick}
            />
            {groupPick === 'new' && (
              <GlassInput
                value={newGroupName}
                placeholder="Group name"
                onChange={(e) => setNewGroupName(e.target.value)}
              />
            )}
          </div>
          <div className={styles.gridTwo}>
            <div className={styles.formField}>
              <span className={styles.label}>First sync: up to</span>
              <GlassInput
                value={initialLimit}
                inputMode="numeric"
                onChange={(e) => setInitialLimit(e.target.value)}
              />
              <span className={styles.helper}>posts downloaded when the subscription first runs</span>
            </div>
            <div className={styles.formField}>
              <span className={styles.label}>Later checks: up to</span>
              <GlassInput
                value={periodicLimit}
                inputMode="numeric"
                onChange={(e) => setPeriodicLimit(e.target.value)}
              />
              <span className={styles.helper}>new posts per check after that</span>
            </div>
          </div>
          <div className={styles.checkboxRow}>
            <ToggleSwitch on={autoCollections} onChange={() => setAutoCollections((v) => !v)} />
            <div>
              <span className={styles.label}>Group multi-image posts</span>
              <span className={styles.helper}>
                Posts with multiple images become a single collection in your library.
              </span>
            </div>
          </div>
        </>
      )}
      </div>
    </GlassModal>
  );
}
