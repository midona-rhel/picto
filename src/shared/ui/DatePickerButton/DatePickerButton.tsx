import { useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { IconCalendar, IconChevronLeft, IconChevronRight } from '@tabler/icons-react';
import styles from './DatePickerButton.module.css';

function parseDate(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  return Number.isNaN(date.getTime()) || dateKey(date) !== value ? null : date;
}

function dateKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function sameDay(left: Date, right: Date): boolean {
  return dateKey(left) === dateKey(right);
}

function calendarDays(month: Date): Date[] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1);
  const start = new Date(first);
  start.setDate(first.getDate() - first.getDay());
  return Array.from({ length: 42 }, (_, index) => (
    new Date(start.getFullYear(), start.getMonth(), start.getDate() + index)
  ));
}

export function DatePickerButton({
  value,
  onChange,
  ariaLabel = 'Choose date',
}: {
  value: string;
  onChange: (value: string) => void;
  ariaLabel?: string;
}) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const selected = useMemo(() => parseDate(value), [value]);
  const [open, setOpen] = useState(false);
  const [month, setMonth] = useState(() => selected ?? new Date());
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const today = new Date();

  const closeOnEscape = (event: React.KeyboardEvent) => {
    if (event.key !== 'Escape') return;
    event.stopPropagation();
    setOpen(false);
    triggerRef.current?.focus();
  };

  const show = () => {
    const nextMonth = selected ?? new Date();
    setMonth(new Date(nextMonth.getFullYear(), nextMonth.getMonth(), 1));
    const rect = triggerRef.current?.getBoundingClientRect();
    if (rect) {
      const width = 278;
      const height = 340;
      setPosition({
        left: Math.max(8, Math.min(rect.left, window.innerWidth - width - 8)),
        top: rect.bottom + height <= window.innerHeight - 8
          ? rect.bottom + 5
          : Math.max(8, rect.top - height - 5),
      });
    }
    setOpen(true);
  };

  const choose = (date: Date) => {
    onChange(dateKey(date));
    setOpen(false);
    triggerRef.current?.focus();
  };

  const days = calendarDays(month);
  const displayValue = selected
    ? selected.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
    : 'Choose date';

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={`${styles.trigger} ${selected ? '' : styles.placeholder}`}
        aria-label={ariaLabel}
        aria-expanded={open}
        onKeyDown={closeOnEscape}
        onClick={() => (open ? setOpen(false) : show())}
      >
        <IconCalendar size={14} stroke={1.8} aria-hidden="true" />
        <span>{displayValue}</span>
      </button>
      {open && createPortal(
        <>
          <button type="button" className={styles.backdrop} aria-label="Close calendar" onClick={() => setOpen(false)} />
          <div
            className={styles.popover}
            style={position}
            role="dialog"
            aria-label={ariaLabel}
            onKeyDown={closeOnEscape}
          >
            <div className={styles.header}>
              <button
                type="button"
                className={styles.navButton}
                aria-label="Previous month"
                onClick={() => setMonth((current) => new Date(current.getFullYear(), current.getMonth() - 1, 1))}
              >
                <IconChevronLeft size={15} stroke={1.8} />
              </button>
              <div className={styles.monthLabel} aria-live="polite">
                {month.toLocaleDateString(undefined, { month: 'long', year: 'numeric' })}
              </div>
              <button
                type="button"
                className={styles.navButton}
                aria-label="Next month"
                onClick={() => setMonth((current) => new Date(current.getFullYear(), current.getMonth() + 1, 1))}
              >
                <IconChevronRight size={15} stroke={1.8} />
              </button>
            </div>
            <div className={styles.weekdays} aria-hidden="true">
              {['S', 'M', 'T', 'W', 'T', 'F', 'S'].map((day, index) => <span key={`${day}-${index}`}>{day}</span>)}
            </div>
            <div className={styles.days}>
              {days.map((day) => {
                const inMonth = day.getMonth() === month.getMonth();
                const isSelected = selected ? sameDay(day, selected) : false;
                const isToday = sameDay(day, today);
                return (
                  <button
                    type="button"
                    key={dateKey(day)}
                    className={`${styles.day} ${inMonth ? '' : styles.outside} ${isToday ? styles.today : ''} ${isSelected ? styles.selected : ''}`}
                    aria-label={day.toLocaleDateString(undefined, { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' })}
                    aria-current={isToday ? 'date' : undefined}
                    aria-pressed={isSelected}
                    onClick={() => choose(day)}
                  >
                    {day.getDate()}
                  </button>
                );
              })}
            </div>
            <div className={styles.footer}>
              <button type="button" className={styles.footerButton} onClick={() => choose(today)}>Today</button>
              <button type="button" className={styles.footerButton} disabled={!selected} onClick={() => { onChange(''); setOpen(false); }}>Clear</button>
            </div>
          </div>
        </>,
        document.body,
      )}
    </>
  );
}
