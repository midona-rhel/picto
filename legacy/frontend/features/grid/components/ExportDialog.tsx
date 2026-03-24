import { Button, Checkbox, Group, Modal, NumberInput, Select, Stack, Text, TextInput } from '@mantine/core';

import { glassModalStyles } from '../../../shared/styles/glassModal';
import type { ExportDialogState } from '../hooks/useGridExportActions';

export function ExportDialog(props: {
  opened: boolean;
  state: ExportDialogState;
  onClose: () => void;
  onChange: (patch: Partial<ExportDialogState>) => void;
  onChooseOutputDir: () => Promise<string | null>;
  onConfirm: () => void | Promise<void>;
  canConfirm: boolean;
}) {
  const {
    opened,
    state,
    onClose,
    onChange,
    onChooseOutputDir,
    onConfirm,
    canConfirm,
  } = props;

  return (
    <Modal opened={opened} onClose={onClose} title="Export As" centered styles={glassModalStyles} size="md">
      <Stack gap="md">
        <Stack gap={6}>
          <Text size="sm" fw={600}>Destination</Text>
          <Group gap="xs" align="end">
            <TextInput
              value={state.outputDir}
              onChange={(event) => onChange({ outputDir: event.currentTarget.value })}
              placeholder="Choose export destination"
              style={{ flex: 1 }}
            />
            <Button variant="default" onClick={() => { void onChooseOutputDir(); }}>
              Choose Folder
            </Button>
          </Group>
        </Stack>

        <Group grow>
          <Checkbox
            checked={state.originalFormat}
            onChange={(event) => onChange({ originalFormat: event.currentTarget.checked })}
            label="Export original format"
          />
        </Group>

        <Group grow>
          <Select
            label="Format"
            size="xs"
            data={[
              { value: 'jpg', label: 'JPEG' },
              { value: 'png', label: 'PNG' },
              { value: 'webp', label: 'WebP' },
              { value: 'avif', label: 'AVIF' },
            ]}
            value={state.format}
            onChange={(value) => {
              if (value) onChange({ format: value as ExportDialogState['format'] });
            }}
            allowDeselect={false}
            disabled={state.originalFormat}
          />
          <NumberInput
            label="Quality"
            size="xs"
            min={1}
            max={100}
            value={state.quality}
            onChange={(value) => onChange({ quality: typeof value === 'number' ? value : 82 })}
            disabled={state.originalFormat || state.format === 'png'}
          />
        </Group>

        <Group grow>
          <NumberInput
            label="Width"
            size="xs"
            min={1}
            value={state.width ?? undefined}
            onChange={(value) => onChange({ width: typeof value === 'number' ? value : null })}
            placeholder="Keep original"
            disabled={state.originalFormat}
          />
          <NumberInput
            label="Height"
            size="xs"
            min={1}
            value={state.height ?? undefined}
            onChange={(value) => onChange({ height: typeof value === 'number' ? value : null })}
            placeholder="Keep original"
            disabled={state.originalFormat}
          />
        </Group>

        <Checkbox
          checked={state.keepAspect}
          onChange={(event) => onChange({ keepAspect: event.currentTarget.checked })}
          label="Keep aspect ratio"
          disabled={state.originalFormat}
        />

        <Text size="xs" c="dimmed">
          {state.originalFormat
            ? 'Original format copies the source files without conversion or resizing.'
            : 'Leave width or height empty to keep the original dimension on that axis.'}
        </Text>

        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>Cancel</Button>
          <Button onClick={() => { void onConfirm(); }} disabled={!canConfirm}>Export</Button>
        </Group>
      </Stack>
    </Modal>
  );
}
