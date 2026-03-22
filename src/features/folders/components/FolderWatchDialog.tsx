import { useEffect, useMemo, useState } from 'react';
import { Button, Group, Modal, Select, Stack, Switch, Text, TextInput } from '@mantine/core';
import { open } from '#desktop/api';
import { notifyError, notifySuccess } from '../../../shared/lib/notify';
import { useDomainStore } from '../../../state/domainStore';
import { useFolderWatchActionStore } from '../../../state/folderWatchActionStore';
import { useTaskStore } from '../../../state/taskStore';
import { getFolderWatchMeta, parseFolderId } from '../../sidebar/lib/folderTreeData';
import { glassModalStyles } from '../../../shared/styles/glassModal';
import { foldersController } from '../../../controllers/foldersController';

type WatchImportStatusMode = 'inherit' | 'inbox' | 'active';

export function FolderWatchDialog() {
  const folderNodes = useDomainStore((s) => s.folderNodes);
  const requestToken = useFolderWatchActionStore((s) => s.requestToken);
  const handledToken = useFolderWatchActionStore((s) => s.handledToken);
  const requestedFolderId = useFolderWatchActionStore((s) => s.folderId);
  const markHandled = useFolderWatchActionStore((s) => s.markHandled);
  const closeRequest = useFolderWatchActionStore((s) => s.close);

  const [opened, setOpened] = useState(false);
  const [folderId, setFolderId] = useState<number | null>(null);
  const [folderName, setFolderName] = useState('');
  const [watchPath, setWatchPath] = useState('');
  const [watchEnabled, setWatchEnabled] = useState(true);
  const [watchSubfolders, setWatchSubfolders] = useState(true);
  const [statusMode, setStatusMode] = useState<WatchImportStatusMode>('inherit');
  const [importExistingNow, setImportExistingNow] = useState(false);
  const [saving, setSaving] = useState(false);

  const folderNode = useMemo(() => {
    if (folderId == null) return null;
    return folderNodes.find((node) => parseFolderId(node.id) === folderId) ?? null;
  }, [folderId, folderNodes]);

  useEffect(() => {
    if (requestToken === handledToken || requestedFolderId == null) return;
    markHandled(requestToken);
    const node = folderNodes.find((entry) => parseFolderId(entry.id) === requestedFolderId);
    const watchMeta = node ? getFolderWatchMeta(node) : null;
    setFolderId(requestedFolderId);
    setFolderName(node?.name ?? 'Folder');
    setWatchPath(watchMeta?.watchPath ?? '');
    setWatchEnabled(watchMeta?.watchPath ? watchMeta.watchEnabled : true);
    setWatchSubfolders(watchMeta?.watchPath ? watchMeta.watchSubfolders : true);
    setStatusMode((watchMeta?.watchImportStatusMode ?? 'inherit') as WatchImportStatusMode);
    setImportExistingNow(false);
    setOpened(true);
  }, [folderNodes, handledToken, markHandled, requestToken, requestedFolderId]);

  const handleClose = () => {
    setOpened(false);
    setImportExistingNow(false);
    closeRequest();
  };

  const handleBrowse = async () => {
    try {
      const selected = await open({
        properties: ['openDirectory'],
        message: 'Select a folder to watch',
      });
      const pickedPath = Array.isArray(selected) ? selected[0] : selected;
      if (pickedPath) {
        setWatchPath(pickedPath);
      }
    } catch (error) {
      notifyError(error, 'Pick Watched Folder Failed');
    }
  };

  const handleSave = async () => {
    if (folderId == null) return;
    if (!watchPath.trim()) {
      notifyError(new Error('No folder selected'), 'Pick a Folder First');
      return;
    }
    setSaving(true);
    try {
      if (importExistingNow) {
        useTaskStore.getState().startFamily('import', 'Adding files');
      }
      await foldersController.setWatchConfig({
        folder_id: folderId,
        watch_path: watchPath.trim(),
        watch_enabled: watchEnabled,
        watch_subfolders: watchSubfolders,
        watch_import_status_mode: statusMode,
        import_existing_now: importExistingNow,
      });
      if (importExistingNow) {
        useTaskStore.getState().finishFamily('import');
      }
      notifySuccess('Watched folder saved', 'Folders');
      handleClose();
    } catch (error) {
      if (importExistingNow) {
        useTaskStore.getState().failFamily('import');
      }
      notifyError(error, 'Save Watched Folder Failed');
    } finally {
      setSaving(false);
    }
  };

  const handleRemove = async () => {
    if (folderId == null) return;
    setSaving(true);
    try {
      await foldersController.clearWatchConfig(folderId);
      notifySuccess('Watched folder removed', 'Folders');
      handleClose();
    } catch (error) {
      notifyError(error, 'Remove Watched Folder Failed');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      opened={opened}
      onClose={handleClose}
      title="Auto-Import Folder"
      centered
      size="md"
      styles={glassModalStyles}
    >
      <Stack gap="md">
        <Text size="sm" c="dimmed">
          Attach a disk folder to <strong>{folderName}</strong>. New files dropped there will be imported into Picto automatically.
        </Text>

        <Stack gap={6}>
          <Text size="sm" fw={500}>Watched disk folder</Text>
          <Group gap="sm" align="end" wrap="nowrap">
            <TextInput
              value={watchPath}
              readOnly
              placeholder="No folder selected"
              style={{ flex: 1 }}
            />
            <Button variant="default" onClick={handleBrowse}>
              Browse
            </Button>
          </Group>
        </Stack>

        <Switch
          label="Enable watch"
          checked={watchEnabled}
          onChange={(event) => setWatchEnabled(event.currentTarget.checked)}
        />

        <Switch
          label="Watch subfolders"
          checked={watchSubfolders}
          onChange={(event) => setWatchSubfolders(event.currentTarget.checked)}
        />

        <Select
          label="Import status"
          data={[
            { value: 'inherit', label: 'Use global default' },
            { value: 'inbox', label: 'Inbox' },
            { value: 'active', label: 'Active' },
          ]}
          value={statusMode}
          onChange={(value) => setStatusMode((value as WatchImportStatusMode | null) ?? 'inherit')}
          allowDeselect={false}
        />

        <Switch
          label="Import existing contents now"
          checked={importExistingNow}
          onChange={(event) => setImportExistingNow(event.currentTarget.checked)}
        />

        <Text size="xs" c="dimmed">
          With subfolder watching enabled, Picto will mirror the disk subtree under the bound folder.
        </Text>

        <Group justify="space-between">
          <Button
            variant="subtle"
            color="red"
            onClick={handleRemove}
            disabled={!folderNode || !getFolderWatchMeta(folderNode).watchPath || saving}
          >
            Remove
          </Button>
          <Group gap="sm">
            <Button variant="default" onClick={handleClose} disabled={saving}>
              Cancel
            </Button>
            <Button onClick={handleSave} loading={saving}>
              Save
            </Button>
          </Group>
        </Group>
      </Stack>
    </Modal>
  );
}
