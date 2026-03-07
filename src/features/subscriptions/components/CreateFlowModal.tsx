import { useState, useEffect } from 'react';
import { Modal, Select, Stack, Text, TextInput } from '@mantine/core';
import { TextButton } from '../../../shared/components/TextButton';
import { glassModalStyles } from '../../../shared/styles/glassModal';
import { notifySuccess, notifyError } from '../../../shared/lib/notify';
import { SubscriptionController } from '../../../controllers/subscriptionController';
import { SCHEDULE_OPTIONS } from '../types';

export function CreateFlowModal({
  opened,
  onClose,
  onCreated,
}: {
  opened: boolean;
  onClose: () => void;
  onCreated?: () => void;
}) {
  const [name, setName] = useState('');
  const [schedule, setSchedule] = useState('manual');
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (opened) {
      setName('');
      setSchedule('manual');
    }
  }, [opened]);

  const handleCreate = async () => {
    if (!name.trim()) return;
    try {
      setLoading(true);
      await SubscriptionController.createFlow({
        name: name.trim(),
        schedule: schedule !== 'manual' ? schedule : undefined,
      });

      notifySuccess(`"${name.trim()}" created. Add one or more queries in this subscription.`, 'Subscription Created');
      onCreated?.();
      onClose();
    } catch (error) {
      notifyError(`Failed to create: ${error}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <Modal opened={opened} onClose={onClose} title="New Subscription" size="sm" styles={glassModalStyles}>
      <Stack gap="md">
        <TextInput
          label="Name"
          placeholder="e.g., Artists Daily"
          value={name}
          onChange={(e) => setName(e.target.value)}
          disabled={loading}
          data-autofocus
        />
        <Select
          label="Schedule"
          value={schedule}
          onChange={(value) => { if (value) setSchedule(value); }}
          data={SCHEDULE_OPTIONS}
          size="xs"
          allowDeselect={false}
          disabled={loading}
        />
        <Text size="xs" c="dimmed">
          A subscription can contain multiple site-specific queries. Add queries after creating it.
        </Text>
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 6 }}>
          <TextButton onClick={onClose} disabled={loading}>Cancel</TextButton>
          <TextButton onClick={handleCreate} disabled={!name.trim() || loading}>
            Create Flow
          </TextButton>
        </div>
      </Stack>
    </Modal>
  );
}
