import {
  useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties,
} from 'react';
import { IconChevronLeft, IconChevronRight, IconQuestionMark } from '@tabler/icons-react';
import { WindowCloseButton } from '../../shared/ui/WindowControls';
import { GUIDED_TOUR_STEPS, type TutorialPlacement } from './tutorialSteps';
import {
  executeTutorialActions, finishTutorialSession, startTutorialSession, waitForTutorialCondition,
} from './tutorialRuntime';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import styles from './HelpOverlay.module.css';

type HelpMode = 'closed' | 'launcher' | 'tour';
interface RectSnapshot { top: number; left: number; right: number; bottom: number; width: number; height: number }
const CARD_WIDTH = 320;
const CARD_HEIGHT = 244;
const CARD_GAP = 14;
const VIEWPORT_MARGIN = 10;

function anchors(id: string): HTMLElement[] {
  const precise = Array.from(document.querySelectorAll<HTMLElement>(`[data-help-rect="${id}"]`));
  return precise.length > 0 ? precise : Array.from(document.querySelectorAll<HTMLElement>(
    `[data-help-id="${id}"], [data-help-region="${id}"]`,
  ));
}

function anchorRect(id: string): RectSnapshot | null {
  const rects = anchors(id).map((element) => element.getBoundingClientRect()).filter((rect) => rect.width > 0 && rect.height > 0);
  if (rects.length === 0) return null;
  const top = Math.max(0, Math.min(...rects.map((rect) => rect.top)));
  const left = Math.max(0, Math.min(...rects.map((rect) => rect.left)));
  const right = Math.min(window.innerWidth, Math.max(...rects.map((rect) => rect.right)));
  const bottom = Math.min(window.innerHeight, Math.max(...rects.map((rect) => rect.bottom)));
  return { top, left, right, bottom, width: right - left, height: bottom - top };
}

function positionCard(target: RectSnapshot, preferred: TutorialPlacement) {
  const order: TutorialPlacement[] = [preferred, 'right', 'left', 'bottom', 'top'];
  const fits = (placement: TutorialPlacement) => {
    if (placement === 'right') return target.right + CARD_GAP + CARD_WIDTH <= window.innerWidth - VIEWPORT_MARGIN;
    if (placement === 'left') return target.left - CARD_GAP - CARD_WIDTH >= VIEWPORT_MARGIN;
    if (placement === 'bottom') return target.bottom + CARD_GAP + CARD_HEIGHT <= window.innerHeight - VIEWPORT_MARGIN;
    return target.top - CARD_GAP - CARD_HEIGHT >= VIEWPORT_MARGIN;
  };
  const placement = order.find(fits) ?? preferred;
  let top = target.top + target.height / 2 - CARD_HEIGHT / 2;
  let left = target.left + target.width / 2 - CARD_WIDTH / 2;
  if (placement === 'right') left = target.right + CARD_GAP;
  if (placement === 'left') left = target.left - CARD_GAP - CARD_WIDTH;
  if (placement === 'bottom') top = target.bottom + CARD_GAP;
  if (placement === 'top') top = target.top - CARD_GAP - CARD_HEIGHT;
  return {
    placement,
    top: Math.max(VIEWPORT_MARGIN, Math.min(top, window.innerHeight - CARD_HEIGHT - VIEWPORT_MARGIN)),
    left: Math.max(VIEWPORT_MARGIN, Math.min(left, window.innerWidth - CARD_WIDTH - VIEWPORT_MARGIN)),
  };
}

export function HelpOverlay({ onPracticeChange }: { onPracticeChange?: (active: boolean) => void } = {}) {
  const [mode, setMode] = useState<HelpMode>('closed');
  const [index, setIndex] = useState(0);
  const [targetRect, setTargetRect] = useState<RectSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const closingRef = useRef(false);
  const active = GUIDED_TOUR_STEPS[index];

  useEffect(() => onPracticeChange?.(mode === 'tour'), [mode, onPracticeChange]);
  useShortcutScope((event) => {
    if (mode === 'closed') {
      const target = event.target;
      const editing = target instanceof HTMLElement && target.matches('input, textarea, [contenteditable="true"]');
      if (event.key !== '?' || event.metaKey || event.ctrlKey || event.altKey || editing) return false;
      setMode('launcher');
      return true;
    }
    if (mode !== 'tour') return false;
    if ((event.target as HTMLElement | null)?.closest?.('[data-tutorial-controls]')) return false;
    return true;
  }, { priority: 1_000, allowInEditable: true });
  const measure = useCallback(() => setTargetRect(mode === 'tour' && active ? anchorRect(active.target) : null), [active, mode]);
  useLayoutEffect(measure, [measure]);
  useEffect(() => {
    if (mode !== 'tour' || !active) return;
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    anchors(active.target).forEach((target) => observer?.observe(target));
    window.addEventListener('resize', measure);
    window.addEventListener('scroll', measure, true);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', measure);
      window.removeEventListener('scroll', measure, true);
    };
  }, [active, measure, mode]);

  useEffect(() => {
    if (mode !== 'tour') return;
    const block = (event: KeyboardEvent) => {
      if ((event.target as HTMLElement | null)?.closest?.('[data-tutorial-controls]')) return;
      event.preventDefault();
      event.stopImmediatePropagation();
    };
    window.addEventListener('keyup', block, true);
    return () => {
      window.removeEventListener('keyup', block, true);
    };
  }, [mode]);

  const prepareStep = useCallback(async (nextIndex: number) => {
    const next = GUIDED_TOUR_STEPS[nextIndex];
    if (!next) return;
    setBusy(true);
    setError(null);
    try {
      await executeTutorialActions(next.enter);
      await waitForTutorialCondition(next.waitFor);
      setIndex(nextIndex);
      requestAnimationFrame(measure);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  }, [measure]);

  const start = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await startTutorialSession();
      setMode('tour');
      setIndex(0);
      await executeTutorialActions(GUIDED_TOUR_STEPS[0].enter);
      await waitForTutorialCondition(GUIDED_TOUR_STEPS[0].waitFor);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setMode('launcher');
    } finally {
      setBusy(false);
    }
  }, []);

  const exit = useCallback(async () => {
    if (closingRef.current) return;
    closingRef.current = true;
    setBusy(true);
    setError(null);
    try {
      if (mode === 'tour') await finishTutorialSession();
      setMode('closed');
      setIndex(0);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      closingRef.current = false;
      setBusy(false);
    }
  }, [mode]);

  const previous = async () => {
    if (index === 0 || busy) return;
    await executeTutorialActions(active.leaveBackward);
    await prepareStep(index - 1);
  };
  const next = async () => {
    if (busy) return;
    if (index === GUIDED_TOUR_STEPS.length - 1) await exit();
    else await prepareStep(index + 1);
  };

  const card = active ? (targetRect ? positionCard(targetRect, active.placement) : {
    placement: active.placement,
    top: Math.max(VIEWPORT_MARGIN, (window.innerHeight - CARD_HEIGHT) / 2),
    left: Math.max(VIEWPORT_MARGIN, (window.innerWidth - CARD_WIDTH) / 2),
  }) : null;
  const pointerStyle = targetRect && card ? (() => {
    if (card.placement === 'right') return { left: targetRect.right + 4, top: targetRect.top + targetRect.height / 2 - 7 };
    if (card.placement === 'left') return { left: targetRect.left - 18, top: targetRect.top + targetRect.height / 2 - 7 };
    if (card.placement === 'bottom') return { left: targetRect.left + targetRect.width / 2 - 7, top: targetRect.bottom + 4 };
    return { left: targetRect.left + targetRect.width / 2 - 7, top: targetRect.top - 18 };
  })() : null;

  return (
    <>
      {mode === 'closed' && (
        <button className={styles.helpButton} type="button" aria-label="Help and tutorial" onClick={() => setMode('launcher')}>
          <IconQuestionMark size={16} stroke={2} />
        </button>
      )}
      {mode !== 'closed' && (
        <div className={styles.overlay} data-testid="help-overlay" onPointerDown={(event) => event.preventDefault()}>
          {mode === 'tour' && targetRect && (
            <svg className={styles.spotlight} aria-hidden="true">
              <defs>
                <filter id="tutorial-cutout-soften" x="-25%" y="-25%" width="150%" height="150%"><feGaussianBlur stdDeviation="9" /></filter>
                <mask id="tutorial-cutout-mask">
                  <rect width="100%" height="100%" fill="white" />
                  <rect x={targetRect.left} y={targetRect.top} width={targetRect.width} height={targetRect.height} rx="14" fill="black" filter="url(#tutorial-cutout-soften)" />
                  <rect x={targetRect.left} y={targetRect.top} width={targetRect.width} height={targetRect.height} rx="14" fill="black" />
                </mask>
              </defs>
              <rect className={styles.spotlightShade} width="100%" height="100%" mask="url(#tutorial-cutout-mask)" />
            </svg>
          )}
          {mode === 'tour' && pointerStyle && card && <span className={styles.targetPointer} data-placement={card.placement} style={pointerStyle} />}
          {mode === 'launcher' && (
            <section className={styles.exploreCard} data-tutorial-controls onPointerDown={(event) => event.stopPropagation()}>
              <div className={styles.closeButton}><WindowCloseButton ariaLabel="Close help" onClick={() => setMode('closed')} /></div>
              <strong>Explore Picto</strong>
              <span>The guided tour opens a temporary offline library and restores this one when you exit.</span>
              {error && <span className={styles.error}>{error}</span>}
              <button className={styles.primaryButton} type="button" disabled={busy} onClick={() => void start()}>{busy ? 'Preparing…' : 'Start guided tour'}</button>
            </section>
          )}
          {mode === 'tour' && active && card && (
            <section className={styles.coachmark} data-tutorial-controls style={{ top: card.top, left: card.left } as CSSProperties} role="dialog" aria-modal="true" aria-labelledby="picto-help-title" onPointerDown={(event) => event.stopPropagation()}>
              <div className={styles.closeButton}><WindowCloseButton ariaLabel="Exit tutorial" disabled={busy} onClick={() => void exit()} /></div>
              <span className={styles.progress}>{active.chapter.replace('-', ' ')} · {index + 1} of {GUIDED_TOUR_STEPS.length}</span>
              <h2 id="picto-help-title">{active.title}</h2>
              <p>{busy ? 'Preparing the real interface…' : active.description}</p>
              {error && <p className={styles.error}>{error}</p>}
              <div className={styles.actions}>
                <button type="button" disabled={busy || index === 0} onClick={() => void previous()}><IconChevronLeft size={14} /> Previous</button>
                <button type="button" disabled={busy} onClick={() => void exit()}>Skip</button>
                <button type="button" disabled={busy} onClick={() => void next()}>{index === GUIDED_TOUR_STEPS.length - 1 ? 'Exit' : <>Next <IconChevronRight size={14} /></>}</button>
              </div>
            </section>
          )}
        </div>
      )}
    </>
  );
}
