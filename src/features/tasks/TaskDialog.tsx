/**
 * The foreground task view: a heavy job (an import) takes the front of the
 * app with its progress, an estimate, Pause and Stop. It can be sent to the
 * background, where it continues in the task bar until reopened.
 */
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useAtomValue } from 'jotai';
import {
  IconAlertTriangle,
  IconArrowsDiagonal,
  IconArrowsDiagonalMinimize2,
  IconCheck,
  IconFolderSearch,
  IconPhotoDown,
  IconPlayerPause,
  IconPlayerPlay,
  IconPlayerStop,
} from '@tabler/icons-react';
import {
  dismissTask,
  pauseTask,
  resumeTask,
  stopTask,
  taskFailures,
} from '../../platform/taskApi';
import {
  applyTasks,
  isTaskOpen,
  minimizeTask,
  showTask,
  taskPresentationAtom,
  tasksAtom,
} from '../../runtime/taskSettle';
import { useShortcutSuspension } from '../../shared/hooks/useShortcutScope';
import { showErrorNotification } from '../../shared/lib/notifications';
import btnStyles from '../../shared/styles/actionButton.module.css';
import type { ForegroundTask } from '../../shared/types/generated/application/ForegroundTask';
import type { TaskFailure } from '../../shared/types/generated/application/TaskFailure';
import { t } from '../../i18n';
import {
  formatCount,
  formatTimeLeft,
  formatTimeLeftShort,
  taskPhase,
  taskSubtitle,
  taskTitle,
  taskTone,
} from './taskPresentation';
import styles from './TaskDialog.module.css';

async function control(action: () => Promise<ForegroundTask[]>, failure: string) {
  try {
    applyTasks(await action());
  } catch (reason) {
    showErrorNotification({
      title: failure,
      message: reason instanceof Error ? reason.message : String(reason),
    });
  }
}

function TaskIcon({ task }: { task: ForegroundTask }) {
  const phase = taskPhase(task);
  const Icon = phase === 'finding' ? IconFolderSearch
    : phase === 'paused' ? IconPlayerPause
      : phase === 'finished' ? IconCheck
        : phase === 'stopped' || phase === 'undone' ? (task.error ? IconAlertTriangle : IconPlayerStop)
          : IconPhotoDown;
  return (
    <span className={styles.kind} data-tone={taskTone(task)} aria-hidden="true">
      <Icon size={17} stroke={1.75} />
    </span>
  );
}

function Progress({ task }: { task: ForegroundTask }) {
  const phase = taskPhase(task);
  const indeterminate = phase === 'finding' || phase === 'removing' || (task.total === 0 && phase === 'importing');
  const percent = task.total > 0 ? Math.min(100, (task.done / task.total) * 100) : 0;
  return (
    <div
      className={styles.track}
      role="progressbar"
      aria-valuemin={indeterminate ? undefined : 0}
      aria-valuemax={indeterminate ? undefined : 100}
      aria-valuenow={indeterminate ? undefined : Math.round(percent)}
    >
      <div
        className={styles.fill}
        data-tone={taskTone(task)}
        data-indeterminate={indeterminate || undefined}
        style={indeterminate ? undefined : { width: `${phase === 'finished' ? 100 : percent}%` }}
      />
    </div>
  );
}

function Counts({ task }: { task: ForegroundTask }) {
  const phase = taskPhase(task);
  let left: string;
  let right: string | null;
  if (phase === 'finding') {
    left = t('{value0} files found', { value0: formatCount(task.total) });
    right = t('Starting…');
  } else if (phase === 'finished' || phase === 'stopped' || phase === 'undone') {
    left = t('{value0} added', { value0: formatCount(task.added) });
    right = t('{value0} files', { value0: formatCount(task.total) });
  } else {
    left = t('{value0} of {value1} files', { value0: formatCount(task.done), value1: formatCount(task.total) });
    right = phase === 'paused' ? t('Paused')
      : phase === 'removing' ? null
        : formatTimeLeft(task.eta_seconds) ?? t('Estimating…');
  }
  return (
    <>
      <div className={styles.counts}>
        <span>{left}</span>
        {right ? <span>{right}</span> : null}
      </div>
      {task.existing > 0 || task.failed > 0 ? (
        <div className={styles.stats}>
          {task.existing > 0 ? <span>{t('{value0} already in library', { value0: formatCount(task.existing) })}</span> : null}
          {task.failed > 0 ? <span className={styles.warn}>{t('{value0} failed', { value0: formatCount(task.failed) })}</span> : null}
        </div>
      ) : null}
    </>
  );
}

function Failures({ taskId }: { taskId: number }) {
  const [failures, setFailures] = useState<TaskFailure[] | null>(null);
  useEffect(() => {
    let cancelled = false;
    void taskFailures(taskId).then((rows) => { if (!cancelled) setFailures(rows); }).catch(() => {
      if (!cancelled) setFailures([]);
    });
    return () => { cancelled = true; };
  }, [taskId]);
  if (!failures) return null;
  return (
    <ul className={styles.failures} aria-label={t('Files that could not be imported')}>
      {failures.map((failure) => (
        <li key={failure.path}>
          <span className={styles.failurePath} title={failure.path}>{failure.path.split(/[\\/]/).pop()}</span>
          <span className={styles.failureError}>{failure.error}</span>
        </li>
      ))}
    </ul>
  );
}

type StopChoice = 'keep' | 'remove';

function StopView({
  task,
  onCancel,
  onStop,
}: {
  task: ForegroundTask;
  onCancel: () => void;
  onStop: (choice: StopChoice) => void;
}) {
  const [choice, setChoice] = useState<StopChoice>('keep');
  const remaining = Math.max(0, task.total - task.done);
  const options: Array<{ id: StopChoice; title: string; detail: string }> = [
    {
      id: 'keep',
      title: t('Keep what was imported'),
      detail: t('The {value0} new items stay. The remaining {value1} files are not imported.', {
        value0: formatCount(task.added),
        value1: formatCount(remaining),
      }),
    },
  ];
  if (task.revertible) {
    options.push({
      id: 'remove',
      title: t('Remove what was imported'),
      detail: t('Deletes the {value0} items this import added. Items that were already in your library are left alone.', {
        value0: formatCount(task.added),
      }),
    });
  }
  return (
    <>
      <div className={styles.body}>
        <div className={styles.head}>
          <span className={styles.kind} data-tone="paused" aria-hidden="true"><IconPlayerStop size={17} stroke={1.75} /></span>
          <div className={styles.titles}>
            <div className={styles.title}>{t('Stop importing “{value0}”?', { value0: task.title })}</div>
            <div className={styles.subtitle}>
              {t('{value0} of {value1} files are done so far', { value0: formatCount(task.done), value1: formatCount(task.total) })}
            </div>
          </div>
        </div>
        {options.length > 1 ? (
          <div className={styles.choices} role="radiogroup" aria-label={t('When stopping')}>
            {options.map((option) => (
              <button
                key={option.id}
                type="button"
                role="radio"
                aria-checked={choice === option.id}
                className={styles.choice}
                data-selected={choice === option.id || undefined}
                data-danger={option.id === 'remove' || undefined}
                onClick={() => setChoice(option.id)}
              >
                <span className={styles.choiceText}>
                  <span className={styles.choiceTitle}>{option.title}</span>
                  <span className={styles.choiceDetail}>{option.detail}</span>
                </span>
                <span className={styles.choiceMark} aria-hidden="true">
                  {choice === option.id ? <IconCheck size={14} stroke={2.25} /> : null}
                </span>
              </button>
            ))}
          </div>
        ) : (
          <p className={styles.note}>{options[0].detail}</p>
        )}
      </div>
      <div className={styles.foot}>
        <span className={styles.spacer} />
        <button type="button" className={btnStyles.btn} onClick={onCancel}>{t('Keep importing')}</button>
        <button
          type="button"
          className={`${btnStyles.btn} ${choice === 'remove' ? btnStyles.btnDanger : btnStyles.btnPrimary}`}
          data-modal-primary="true"
          onClick={() => onStop(choice)}
        >
          {choice === 'remove' ? t('Stop and remove') : t('Stop import')}
        </button>
      </div>
    </>
  );
}

function TaskView({ task }: { task: ForegroundTask }) {
  const phase = taskPhase(task);
  const [stopping, setStopping] = useState<{ resume: boolean } | null>(null);
  const [showFailures, setShowFailures] = useState(false);
  const open = isTaskOpen(task);

  useEffect(() => {
    if (!open) setStopping(null);
  }, [open]);

  if (stopping && open) {
    return (
      <StopView
        task={task}
        onCancel={() => {
          setStopping(null);
          if (stopping.resume) void control(() => resumeTask(task.task_id), t('Could not resume the import'));
        }}
        onStop={(choice) => {
          setStopping(null);
          void control(() => stopTask(task.task_id, choice === 'remove'), t('Could not stop the import'));
        }}
      />
    );
  }

  const ended = !open;
  return (
    <>
      <div className={styles.body} aria-live="polite">
        <div className={styles.head}>
          <TaskIcon task={task} />
          <div className={styles.titles}>
            <div className={styles.title}>{taskTitle(task)}</div>
            <div className={styles.subtitle} data-warning={Boolean(task.error) || undefined}>{taskSubtitle(task)}</div>
          </div>
        </div>
        <Progress task={task} />
        <Counts task={task} />
        {showFailures && task.failed > 0 ? <Failures taskId={task.task_id} /> : null}
      </div>
      <div className={styles.foot}>
        {ended ? (
          task.failed > 0 ? (
            <button type="button" className={`${btnStyles.btn} ${styles.ghost}`} onClick={() => setShowFailures((value) => !value)}>
              <IconAlertTriangle size={14} stroke={1.75} />
              {showFailures ? t('Hide failed') : t('Show {value0} failed', { value0: formatCount(task.failed) })}
            </button>
          ) : null
        ) : (
          <button type="button" className={`${btnStyles.btn} ${styles.ghost}`} onClick={() => minimizeTask(task.task_id)}>
            <IconArrowsDiagonalMinimize2 size={14} stroke={1.75} />
            {t('Run in background')}
          </button>
        )}
        <span className={styles.spacer} />
        {ended ? (
          <button
            type="button"
            className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
            data-modal-primary="true"
            onClick={() => { void control(() => dismissTask(task.task_id), t('Could not close the task')); }}
          >
            {t('Done')}
          </button>
        ) : phase === 'removing' ? null : (
          <>
            {phase === 'paused' ? (
              <button
                type="button"
                className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
                onClick={() => { void control(() => resumeTask(task.task_id), t('Could not resume the import')); }}
              >
                <IconPlayerPlay size={14} stroke={1.75} />
                {t('Resume')}
              </button>
            ) : (
              <button
                type="button"
                className={btnStyles.btn}
                onClick={() => { void control(() => pauseTask(task.task_id), t('Could not pause the import')); }}
              >
                <IconPlayerPause size={14} stroke={1.75} />
                {t('Pause')}
              </button>
            )}
            <button
              type="button"
              className={btnStyles.btn}
              onClick={() => {
                // Nothing more is imported while the user decides.
                const resume = task.state === 'running';
                setStopping({ resume });
                if (resume) void control(() => pauseTask(task.task_id), t('Could not pause the import'));
              }}
            >
              <IconPlayerStop size={14} stroke={1.75} />
              {t('Stop…')}
            </button>
          </>
        )}
      </div>
    </>
  );
}

export function TaskDialog() {
  const tasks = useAtomValue(tasksAtom);
  const { focusedTaskId } = useAtomValue(taskPresentationAtom);
  const task = tasks.find((candidate) => candidate.task_id === focusedTaskId) ?? null;
  const dialogRef = useRef<HTMLDivElement>(null);
  useShortcutSuspension(task != null);

  useEffect(() => {
    if (task == null) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.focus();
    return () => previous?.focus();
  }, [task == null]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!task) return null;
  return createPortal(
    <div className={styles.backdrop}>
      <div
        ref={dialogRef}
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-label={taskTitle(task)}
        tabIndex={-1}
        data-task-dialog
        onKeyDown={(event) => {
          if (event.key === 'Escape' && isTaskOpen(task)) {
            event.preventDefault();
            minimizeTask(task.task_id);
          }
        }}
      >
        <TaskView key={task.task_id} task={task} />
      </div>
    </div>,
    document.body,
  );
}

/** Tasks sent to the background, one row each, above the bottom edge. */
export function TaskBar() {
  const tasks = useAtomValue(tasksAtom);
  const { focusedTaskId, minimized } = useAtomValue(taskPresentationAtom);
  const shown = tasks.filter((task) => minimized.has(task.task_id) && task.task_id !== focusedTaskId);
  if (shown.length === 0) return null;
  return (
    <div className={styles.bar} role="status" aria-live="polite">
      {shown.map((task) => {
        const phase = taskPhase(task);
        const timeLeft = phase === 'importing' ? formatTimeLeftShort(task.eta_seconds) : null;
        return (
          <div key={task.task_id} className={styles.barRow} data-task-bar-row>
            <TaskIcon task={task} />
            <button type="button" className={styles.barTitle} onClick={() => showTask(task.task_id)}>
              {taskTitle(task)}
            </button>
            <Progress task={task} />
            <span className={styles.barDetail}>
              {phase === 'finding'
                ? t('{value0} found', { value0: formatCount(task.total) })
                : `${formatCount(task.done)} / ${formatCount(task.total)}${timeLeft ? ` · ${timeLeft}` : ''}`}
            </span>
            {phase === 'importing' || phase === 'finding' ? (
              <button
                type="button"
                className={styles.iconButton}
                aria-label={t('Pause')}
                onClick={() => { void control(() => pauseTask(task.task_id), t('Could not pause the import')); }}
              >
                <IconPlayerPause size={15} stroke={1.75} />
              </button>
            ) : phase === 'paused' ? (
              <button
                type="button"
                className={styles.iconButton}
                aria-label={t('Resume')}
                onClick={() => { void control(() => resumeTask(task.task_id), t('Could not resume the import')); }}
              >
                <IconPlayerPlay size={15} stroke={1.75} />
              </button>
            ) : null}
            <button
              type="button"
              className={styles.iconButton}
              aria-label={t('Show')}
              onClick={() => showTask(task.task_id)}
            >
              <IconArrowsDiagonal size={15} stroke={1.75} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
