import { useCallback, useMemo, useState } from 'react';
import { Modal, Stack, Group, TextInput, Text, ScrollArea } from '@mantine/core';
import { IconArrowRight } from '@tabler/icons-react';
import { glassModalStyles } from '../../styles/glassModal';
import { TextButton } from '../ui/TextButton';
import { FileController } from '../../controllers/fileController';
import { registerUndoAction } from '../../controllers/undoRedoController';
import { notifySuccess, notifyError } from '../../lib/notify';
import { api } from '#desktop/api';
import type { MasonryImageItem } from '../image-grid/shared';
import classes from './BatchRenameDialog.module.css';

// ── Types ──────────────────────────────────────────────────────────────────

interface BatchRenameDialogProps {
  opened: boolean;
  onClose: () => void;
  images: MasonryImageItem[];
}

type Mode = 'template' | 'regex';

// ── Helpers ────────────────────────────────────────────────────────────────

function getExtension(name: string | null, mime: string): string {
  if (name) {
    const dot = name.lastIndexOf('.');
    if (dot > 0) return name.slice(dot + 1);
  }
  const sub = mime.split('/')[1];
  if (sub === 'jpeg') return 'jpg';
  return sub ?? '';
}

function getBaseName(name: string | null): string {
  if (!name) return '';
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(0, dot) : name;
}

function padIndex(n: number, total: number): string {
  const digits = String(total).length;
  return String(n).padStart(digits, '0');
}

function applyTemplate(
  template: string,
  image: MasonryImageItem,
  index: number,
  total: number,
): string {
  const baseName = getBaseName(image.name);
  const ext = getExtension(image.name, image.mime);
  const date = image.imported_at ? image.imported_at.slice(0, 10) : '';

  let result = template;
  result = result.replace(/\{name\}/gi, baseName);
  result = result.replace(/\{ext\}/gi, ext);
  result = result.replace(/\{date\}/gi, date);

  // {n+N} — index starting from N
  result = result.replace(/\{n\+(\d+)\}/gi, (_match, offset) => {
    return padIndex(index + parseInt(offset, 10), total + parseInt(offset, 10));
  });

  // {n} — 1-based index
  result = result.replace(/\{n\}/gi, padIndex(index + 1, total));

  return result;
}

function applyRegex(
  findPattern: string,
  replaceWith: string,
  name: string,
): string {
  try {
    const re = new RegExp(findPattern, 'g');
    return name.replace(re, replaceWith);
  } catch {
    return name; // Invalid regex — return unchanged
  }
}

// ── Component ──────────────────────────────────────────────────────────────

export function BatchRenameDialog({ opened, onClose, images }: BatchRenameDialogProps) {
  const [mode, setMode] = useState<Mode>('template');
  const [template, setTemplate] = useState('{name}');
  const [findPattern, setFindPattern] = useState('');
  const [replaceWith, setReplaceWith] = useState('');
  const [saving, setSaving] = useState(false);

  const preview = useMemo(() => {
    return images.map((img, i) => {
      const before = img.name ?? img.hash.slice(0, 12);
      let after: string;
      if (mode === 'template') {
        after = applyTemplate(template, img, i, images.length);
      } else {
        after = applyRegex(findPattern, replaceWith, before);
      }
      return { hash: img.hash, before, after, changed: before !== after };
    });
  }, [images, mode, template, findPattern, replaceWith]);

  const changedCount = useMemo(() => preview.filter((p) => p.changed).length, [preview]);

  const handleConfirm = useCallback(async () => {
    const toRename = preview.filter((p) => p.changed);
    if (toRename.length === 0) return;

    setSaving(true);
    try {
      for (const item of toRename) {
        await FileController.setFileName(item.hash, item.after || null);
      }
      registerUndoAction({
        label: `Batch rename ${toRename.length} file(s)`,
        undo: async () => {
          for (const item of toRename) {
            await api.file.setName(item.hash, item.before || null);
          }
        },
        redo: async () => {
          for (const item of toRename) {
            await api.file.setName(item.hash, item.after || null);
          }
        },
      });
      notifySuccess(`Renamed ${toRename.length} file(s)`, 'Batch Rename');
      onClose();
    } catch (err) {
      notifyError(err, 'Batch Rename Failed');
    } finally {
      setSaving(false);
    }
  }, [preview, onClose]);

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title="Batch Rename"
      size="lg"
      centered
      styles={{
        ...glassModalStyles,
        title: { fontWeight: 600, fontSize: 'var(--mantine-font-size-lg)' },
        body: { padding: 'var(--mantine-spacing-lg)' },
      }}
    >
      <Stack gap="md">
        {/* Mode toggle */}
        <div className={classes.modeToggle}>
          <button
            className={`${classes.modeBtn} ${mode === 'template' ? classes.modeBtnActive : ''}`}
            onClick={() => setMode('template')}
          >
            Template
          </button>
          <button
            className={`${classes.modeBtn} ${mode === 'regex' ? classes.modeBtnActive : ''}`}
            onClick={() => setMode('regex')}
          >
            Find &amp; Replace
          </button>
        </div>

        {/* Inputs */}
        {mode === 'template' ? (
          <div>
            <TextInput
              label="Template"
              placeholder="{name}_{n}"
              value={template}
              onChange={(e) => setTemplate(e.currentTarget.value)}
              size="sm"
            />
            <Text className={classes.helpText} mt={6}>
              <span className={classes.helpCode}>{'{name}'}</span> original name,{' '}
              <span className={classes.helpCode}>{'{n}'}</span> index,{' '}
              <span className={classes.helpCode}>{'{n+N}'}</span> index from N,{' '}
              <span className={classes.helpCode}>{'{date}'}</span> import date,{' '}
              <span className={classes.helpCode}>{'{ext}'}</span> extension
            </Text>
          </div>
        ) : (
          <Group grow>
            <TextInput
              label="Find (regex)"
              placeholder="\.png$"
              value={findPattern}
              onChange={(e) => setFindPattern(e.currentTarget.value)}
              size="sm"
            />
            <TextInput
              label="Replace with"
              placeholder=".jpg"
              value={replaceWith}
              onChange={(e) => setReplaceWith(e.currentTarget.value)}
              size="sm"
            />
          </Group>
        )}

        {/* Preview */}
        <div>
          <Text size="sm" fw={500} mb={6}>
            Preview
          </Text>
          <div className={classes.previewHeader}>
            <span>Before</span>
            <span />
            <span>After</span>
          </div>
          <ScrollArea h={200} offsetScrollbars>
            <div className={classes.previewList}>
              {preview.map((row) => (
                <div key={row.hash} className={classes.previewRow}>
                  <span className={classes.previewBefore}>{row.before}</span>
                  <IconArrowRight size={12} className={classes.previewArrow} />
                  <span
                    className={`${classes.previewAfter} ${row.changed ? classes.previewChanged : ''}`}
                  >
                    {row.after}
                  </span>
                </div>
              ))}
            </div>
          </ScrollArea>
        </div>

        {/* Footer */}
        <Group justify="space-between">
          <span className={classes.fileCount}>
            {changedCount} of {images.length} file(s) will be renamed
          </span>
          <div style={{ display: 'flex', gap: 6 }}>
            <TextButton onClick={handleConfirm} disabled={changedCount === 0 || saving}>
              {saving ? 'Renaming...' : 'Rename'}
            </TextButton>
            <TextButton onClick={onClose}>Cancel</TextButton>
          </div>
        </Group>
      </Stack>
    </Modal>
  );
}
