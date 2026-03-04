import { useState } from 'react';
import { Popover, TextInput, SimpleGrid, ActionIcon, Text, ScrollArea } from '@mantine/core';
import { IconSearch } from '@tabler/icons-react';
import { CURATED_ICONS, ICON_CATEGORIES } from './iconRegistry';

interface IconPickerProps {
  value: string | null;
  onChange: (icon: string | null) => void;
  children: React.ReactNode;
}

export function IconPicker({ value, onChange, children }: IconPickerProps) {
  const [opened, setOpened] = useState(false);
  const [search, setSearch] = useState('');

  const query = search.toLowerCase().trim();
  const filtered = query
    ? CURATED_ICONS.filter(
        (i) =>
          i.label.toLowerCase().includes(query) ||
          i.name.toLowerCase().includes(query) ||
          i.category.toLowerCase().includes(query)
      )
    : null;

  const handleSelect = (iconName: string) => {
    onChange(iconName);
    setOpened(false);
    setSearch('');
  };

  const handleClear = () => {
    onChange(null);
    setOpened(false);
    setSearch('');
  };

  return (
    <Popover
      opened={opened}
      onChange={setOpened}
      width={320}
      position="bottom-start"
      shadow="lg"
      withArrow
    >
      <Popover.Target>
        <div style={{ display: 'inline-flex', cursor: 'pointer' }} onClick={() => setOpened((o) => !o)}>
          {children}
        </div>
      </Popover.Target>
      <Popover.Dropdown p="xs">
        <TextInput
          placeholder="Search icons..."
          leftSection={<IconSearch size={14} />}
          size="xs"
          mb="xs"
          value={search}
          onChange={(e) => setSearch(e.currentTarget.value)}
          autoFocus
        />

        {value && (
          <Text
            size="xs"
            c="blue"
            mb="xs"
            style={{ cursor: 'pointer' }}
            onClick={handleClear}
          >
            Clear icon
          </Text>
        )}

        <ScrollArea.Autosize mah={300}>
          {filtered ? (
            filtered.length > 0 ? (
              <SimpleGrid cols={8} spacing={4}>
                {filtered.map((icon) => {
                  const Icon = icon.component;
                  const isSelected = value === icon.name;
                  return (
                    <ActionIcon
                      key={icon.name}
                      variant={isSelected ? 'filled' : 'subtle'}
                      color={isSelected ? 'blue' : 'gray'}
                      size="md"
                      onClick={() => handleSelect(icon.name)}
                      title={icon.label}
                    >
                      <Icon size={16} />
                    </ActionIcon>
                  );
                })}
              </SimpleGrid>
            ) : (
              <Text size="xs" c="dimmed" ta="center" py="sm">
                No icons found
              </Text>
            )
          ) : (
            ICON_CATEGORIES.map((category) => {
              const icons = CURATED_ICONS.filter((i) => i.category === category);
              return (
                <div key={category}>
                  <Text size="xs" c="dimmed" fw={500} mb={4} mt={8}>
                    {category}
                  </Text>
                  <SimpleGrid cols={8} spacing={4}>
                    {icons.map((icon) => {
                      const Icon = icon.component;
                      const isSelected = value === icon.name;
                      return (
                        <ActionIcon
                          key={icon.name}
                          variant={isSelected ? 'filled' : 'subtle'}
                          color={isSelected ? 'blue' : 'gray'}
                          size="md"
                          onClick={() => handleSelect(icon.name)}
                          title={icon.label}
                        >
                          <Icon size={16} />
                        </ActionIcon>
                      );
                    })}
                  </SimpleGrid>
                </div>
              );
            })
          )}
        </ScrollArea.Autosize>
      </Popover.Dropdown>
    </Popover>
  );
}
