import { Text } from '@mantine/core';

interface InfoTextProps {
  label: string;
  value: string;
}

export function InfoText({ label, value }: InfoTextProps) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
      <Text size="xs" c="dimmed">{label}</Text>
      <Text size="xs">{value}</Text>
    </div>
  );
}
