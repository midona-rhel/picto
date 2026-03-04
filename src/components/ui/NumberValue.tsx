import { Text } from '@mantine/core';

interface NumberValueProps {
  label: string;
  value: number | string;
}

export function NumberValue({ label, value }: NumberValueProps) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
      <Text size="xs" c="dimmed">{label}</Text>
      <Text size="xs" fw={600}>{value}</Text>
    </div>
  );
}
