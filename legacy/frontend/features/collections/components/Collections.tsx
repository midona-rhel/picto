import { useState, useEffect } from "react";
import {
  Grid,
  Image,
  Text,
  Group,
  ActionIcon,
  Stack,
  TextInput,
  Badge,
  Modal,
  TagsInput,
  Loader,
  Center,
  Menu,
  Box,
  SegmentedControl,
  Title,
  SimpleGrid,
} from "@mantine/core";
import { glassModalStyles } from '../../../shared/styles/glassModal';
import { useDisclosure } from "@mantine/hooks";
import { notifySuccess, notifyError } from '../../../shared/lib/notify';
import { useNavigationStore } from '../../../state-legacy/navigationStore';
import {
  IconPlus,
  IconSearch,
  IconDotsVertical,
  IconEdit,
  IconTrash,
  IconPhoto,
  IconFolder,
  IconArrowLeft,
  IconUser,
  IconUserCircle,
  IconBooks,
} from "@tabler/icons-react";
import { EmptyState } from '../../../shared/components/EmptyState';
import { TextButton } from '../../../shared/components/TextButton';
import styles from './Collections.module.css';
import {
  MediaCardFrame,
  MediaCardMeta,
  MediaCardOverlay,
} from '../../../shared/components/media-card';
import { collectionsController } from '../../../controllers/collectionsController';
import { companionController } from '../../../controllers/companionController';
import { useCollectionListStore } from '../../../state-legacy/collectionListStore';
import type { CollectionInfo as BackendCollectionInfo } from '../../../shared/types/api';

interface Collection {
  id: number;
  name: string;
  tags: string[];
  image_count: number;
  created_at: string | null;
  updated_at: string | null;
  thumbnail_url?: string;
}

interface CreateCollectionData {
  name: string;
  tags: string[];
}

interface NamespaceCard {
  value: string;
  count: number;
  thumbnail_hash?: string;
}

interface FileImage {
  hash: string;
  size?: number;
  mime?: string;
  width?: number;
  height?: number;
  has_audio?: boolean;
  duration?: number;
  num_frames?: number;
}

type ViewType = 'collections' | 'artist' | 'character' | 'series';

export function Collections() {
  const navigateToCollection = useNavigationStore((s) => s.navigateToCollection);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedTags, setSelectedTags] = useState<string[]>([]);

  // Dynamic view state
  const [viewType, setViewType] = useState<ViewType>('collections');
  const [namespaceCards, setNamespaceCards] = useState<NamespaceCard[]>([]);
  const [selectedNamespaceValue, setSelectedNamespaceValue] = useState<string | null>(null);
  const [filteredImages, setFilteredImages] = useState<FileImage[]>([]);

  // Modal state for create/edit
  const [createModalOpened, { open: openCreateModal, close: closeCreateModal }] = useDisclosure(false);
  const [editModalOpened, { open: openEditModal, close: closeEditModal }] = useDisclosure(false);
  const [editingCollection, setEditingCollection] = useState<Collection | null>(null);

  // Form state
  const [formData, setFormData] = useState<CreateCollectionData>({
    name: "",
    tags: [],
  });

  // Load collections on component mount
  useEffect(() => {
    if (viewType === 'collections') {
      loadCollections();
    } else {
      loadNamespaceValues();
    }
  }, [viewType]);

  // Subscribe to eager collection list mutations from the controller.
  const pendingCollectionRemovals = useCollectionListStore((s) => s.pendingRemovals);
  const pendingCollectionUpdates = useCollectionListStore((s) => s.pendingUpdates);
  const pendingCollectionCreatedId = useCollectionListStore((s) => s.pendingCreatedId);
  useEffect(() => {
    if (pendingCollectionRemovals.length === 0 && pendingCollectionUpdates.length === 0 && pendingCollectionCreatedId == null) return;
    const { removals, updates, createdId } = useCollectionListStore.getState().drainMutations();

    if (removals.length > 0) {
      const removeIds = new Set(removals.map((r) => r.id));
      setCollections((prev) => prev.filter((c) => !removeIds.has(c.id)));
    }

    if (updates.length > 0) {
      setCollections((prev) =>
        prev.map((c) => {
          const update = updates.find((u) => u.id === c.id);
          return update?.name != null ? { ...c, name: update.name } : c;
        }),
      );
    }

    if (createdId != null) {
      // New collection created — fetch the full list to get the new entry with
      // thumbnail and counts. This is the one case where we re-fetch, since the
      // controller doesn't have the full Collection shape at create time.
      loadCollections();
    }
  }, [pendingCollectionRemovals, pendingCollectionUpdates, pendingCollectionCreatedId]);

  // Load data when view changes
  const handleViewTypeChange = (newViewType: ViewType) => {
    setViewType(newViewType);
    setSelectedNamespaceValue(null);
    setFilteredImages([]);
    setSearchQuery("");
  };

  const loadCollections = async () => {
    try {
      setLoading(true);
      const result = await collectionsController.list();
      setCollections(result.map((row: BackendCollectionInfo) => ({
        id: row.id,
        name: row.name,
        tags: row.tags ?? [],
        image_count: row.image_count,
        created_at: row.created_at,
        updated_at: row.updated_at,
        thumbnail_url: row.thumbnail_url ?? undefined,
      })));
    } catch (error) {
      console.error("Failed to load collections:", error);
      notifyError('Failed to load collections');
    } finally {
      setLoading(false);
    }
  };

  const loadNamespaceValues = async () => {
    try {
      setLoading(true);
      const namespace = viewType; // 'artist', 'character', 'series'
      const result = await companionController.getNamespaceValues(namespace) as NamespaceCard[];
      setNamespaceCards(result);
    } catch (error) {
      console.error("Failed to load namespace values:", error);
      notifyError(`Failed to load ${viewType} data`);
    } finally {
      setLoading(false);
    }
  };

  const loadFilteredImages = async (namespaceValue: string) => {
    try {
      setLoading(true);
      const tag = `${viewType}:${namespaceValue}`;
      const result = await companionController.getFilesByTag(tag) as FileImage[];
      setFilteredImages(result);
      setSelectedNamespaceValue(namespaceValue);
    } catch (error) {
      console.error("Failed to load filtered images:", error);
      notifyError(`Failed to load images for ${namespaceValue}`);
    } finally {
      setLoading(false);
    }
  };

  const handleCreateCollection = async () => {
    if (!formData.name.trim()) {
      notifyError('Collection name is required');
      return;
    }

    try {
      await collectionsController.create({
        name: formData.name.trim(),
      });

      notifySuccess('Collection created successfully');

      closeCreateModal();
      resetForm();
      // Controller signals queueCreated → subscription re-fetches list.
    } catch (error) {
      console.error("Failed to create collection:", error);
      notifyError('Failed to create collection');
    }
  };

  const handleEditCollection = async () => {
    if (!editingCollection || !formData.name.trim()) {
      return;
    }

    try {
      await collectionsController.update({
        id: editingCollection.id,
        name: formData.name.trim(),
        tags: formData.tags,
      });

      notifySuccess('Collection updated successfully');

      closeEditModal();
      resetForm();
      // Controller signals queueUpdate → subscription patches name eagerly.
    } catch (error) {
      console.error("Failed to update collection:", error);
      notifyError('Failed to update collection');
    }
  };

  const handleDeleteCollection = async (collection: Collection) => {
    if (!confirm(`Are you sure you want to delete "${collection.name}"? This action cannot be undone.`)) {
      return;
    }

    try {
      await collectionsController.delete(collection.id);

      notifySuccess('Collection deleted successfully');
      // Controller signals queueRemoval → subscription removes eagerly.
    } catch (error) {
      console.error("Failed to delete collection:", error);
      notifyError('Failed to delete collection');
    }
  };

  const resetForm = () => {
    setFormData({
      name: "",
      tags: [],
    });
    setEditingCollection(null);
  };

  const openEditModalWithCollection = (collection: Collection) => {
    setEditingCollection(collection);
    setFormData({
      name: collection.name,
      tags: collection.tags,
    });
    openEditModal();
  };

  // Filter collections based on search and tags
  const filteredCollections = collections.filter((collection) => {
    const matchesSearch = !searchQuery ||
      collection.name.toLowerCase().includes(searchQuery.toLowerCase());

    const matchesTags = selectedTags.length === 0 ||
      selectedTags.some(tag => collection.tags.includes(tag));

    return matchesSearch && matchesTags;
  });

  // Get all available tags for filtering (only for collections view)
  const allTags = Array.from(new Set(collections.flatMap(c => c.tags)));

  // Get the appropriate display info for current view
  const getViewInfo = () => {
    switch (viewType) {
      case 'artist':
        return {
          title: 'Browse by Artist',
          description: 'Browse images grouped by artist tags',
          icon: <IconUser size={20} />
        };
      case 'character':
        return {
          title: 'Browse by Character',
          description: 'Browse images grouped by character tags',
          icon: <IconUserCircle size={20} />
        };
      case 'series':
        return {
          title: 'Browse by Series',
          description: 'Browse images grouped by series tags',
          icon: <IconBooks size={20} />
        };
      default:
        return {
          title: 'My Collections',
          description: 'Organize your images into themed collections',
          icon: <IconFolder size={20} />
        };
    }
  };

  const viewInfo = getViewInfo();

  // Render the main content based on current view state
  const renderMainContent = () => {
    if (loading) {
      return (
        <Center h={400}>
          <Loader size="lg" />
        </Center>
      );
    }

    // If we're viewing filtered images for a specific namespace value
    if (selectedNamespaceValue && filteredImages.length >= 0) {
      return renderFilteredImages();
    }

    // If we're viewing namespace cards (artist, character, series)
    if (viewType !== 'collections') {
      return renderNamespaceCards();
    }

    // Default: collections view
    return renderCollections();
  };

  // Render collections grid
  const renderCollections = () => {
    if (filteredCollections.length === 0) {
      return (
        <EmptyState
          icon={IconFolder}
          title="No collections found"
          description={collections.length === 0
            ? "Create your first collection to get started"
            : "Try adjusting your search or filters"
          }
          action={collections.length === 0 ? (
            <TextButton onClick={openCreateModal}>
              <IconPlus size={14} />
              Create Collection
            </TextButton>
          ) : undefined}
        />
      );
    }

    return (
      <SimpleGrid cols={{ base: 2, sm: 3, md: 4, lg: 5 }}>
        {filteredCollections.map((collection) => (
          <Box
            key={collection.id}
            pos="relative"
            className={styles.card}
            onClick={() => navigateToCollection({ id: collection.id, name: collection.name })}
          >
            <MediaCardFrame>
              {collection.thumbnail_url ? (
                <Image
                  src={collection.thumbnail_url}
                  height={180}
                  alt={collection.name}
                  radius="sm"
                />
              ) : (
                <Center h={180} bg="var(--box-background)" className={styles.placeholder}>
                  <IconPhoto size={32} stroke={1.5} color="gray" />
                </Center>
              )}

              <MediaCardOverlay className={styles.overlayContent}>
                <Group justify="space-between" align="flex-end">
                  <MediaCardMeta>
                    <Text size="sm" fw={500} c="white" lineClamp={1}>{collection.name}</Text>
                    <Text size="xs" c="dimmed">{collection.image_count} images</Text>
                  </MediaCardMeta>
                  <Menu shadow="md" width={160}>
                    <Menu.Target>
                      <ActionIcon color="gray" onClick={(e) => e.stopPropagation()}>
                        <IconDotsVertical size={14} />
                      </ActionIcon>
                    </Menu.Target>
                    <Menu.Dropdown>
                      <Menu.Item
                        leftSection={<IconEdit size={14} />}
                        onClick={(e) => {
                          e.stopPropagation();
                          openEditModalWithCollection(collection);
                        }}
                      >
                        Edit
                      </Menu.Item>
                      <Menu.Item
                        color="red"
                        leftSection={<IconTrash size={14} />}
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDeleteCollection(collection);
                        }}
                      >
                        Delete
                      </Menu.Item>
                    </Menu.Dropdown>
                  </Menu>
                </Group>
              </MediaCardOverlay>

              {collection.tags.length > 0 && (
                <MediaCardOverlay position="top" tone="none" className={styles.topOverlayContent}>
                  <Group gap={4}>
                    {collection.tags.slice(0, 3).map((tag) => (
                      <Badge key={tag} variant="light" size="xs">{tag}</Badge>
                    ))}
                    {collection.tags.length > 3 && (
                      <Badge variant="light" size="xs" color="gray">+{collection.tags.length - 3}</Badge>
                    )}
                  </Group>
                </MediaCardOverlay>
              )}
            </MediaCardFrame>
          </Box>
        ))}
      </SimpleGrid>
    );
  };

  // Render namespace cards (artist, character, series cards)
  const renderNamespaceCards = () => {
    const filteredCards = namespaceCards.filter(card =>
      !searchQuery || card.value.toLowerCase().includes(searchQuery.toLowerCase())
    );

    if (filteredCards.length === 0) {
      return (
        <EmptyState
          iconNode={viewInfo.icon}
          title={`No ${viewType}s found`}
          description={namespaceCards.length === 0
            ? `No ${viewType} tags found in your library`
            : "Try adjusting your search"
          }
        />
      );
    }

    return (
      <SimpleGrid cols={{ base: 2, sm: 3, md: 4, lg: 6 }}>
        {filteredCards.map((card) => (
          <Box
            key={card.value}
            pos="relative"
            className={styles.card}
            onClick={() => loadFilteredImages(card.value)}
          >
            <MediaCardFrame>
              {card.thumbnail_hash ? (
                <Image
                  src={`data:image/jpeg;base64,${card.thumbnail_hash}`}
                  h={120}
                  radius="sm"
                />
              ) : (
                <Center h={120} bg="var(--box-background)" className={styles.placeholder}>
                  <IconPhoto size={24} stroke={1.5} color="gray" />
                </Center>
              )}
              <MediaCardOverlay className={styles.overlayContent}>
                <MediaCardMeta>
                  <Text size="xs" fw={500} c="white" lineClamp={1}>{card.value}</Text>
                  <Text size="xs" c="dimmed">{card.count}</Text>
                </MediaCardMeta>
              </MediaCardOverlay>
            </MediaCardFrame>
          </Box>
        ))}
      </SimpleGrid>
    );
  };

  // Render filtered images for a specific namespace value
  const renderFilteredImages = () => {
    const filteredImageList = filteredImages.filter(image =>
      !searchQuery || image.hash.toLowerCase().includes(searchQuery.toLowerCase())
    );

    if (filteredImageList.length === 0) {
      return (
        <EmptyState
          icon={IconPhoto}
          title="No images found"
          description={filteredImages.length === 0
            ? `No images found for ${selectedNamespaceValue}`
            : "Try adjusting your search"
          }
        />
      );
    }

    return (
      <Stack gap="md">
        <Group justify="space-between">
          <Title order={3}>{selectedNamespaceValue}</Title>
          <Text c="dimmed" size="sm">
            {filteredImageList.length} of {filteredImages.length} images
          </Text>
        </Group>

        <SimpleGrid cols={{ base: 3, sm: 4, md: 5, lg: 8 }}>
          {filteredImageList.map((image) => (
            <Box key={image.hash} pos="relative" className={styles.card}>
              <MediaCardFrame>
                <Center h={120} bg="var(--box-background)" className={styles.placeholder}>
                  <IconPhoto size={24} stroke={1.5} color="gray" />
                </Center>
                <MediaCardOverlay tone="soft" className={styles.filteredOverlayContent}>
                  <Text size="xs" c="dimmed" lineClamp={1}>
                    {image.hash.substring(0, 8)}
                    {image.width && image.height ? ` · ${image.width}×${image.height}` : ''}
                  </Text>
                </MediaCardOverlay>
              </MediaCardFrame>
            </Box>
          ))}
        </SimpleGrid>
      </Stack>
    );
  };

  return (
    <div className={styles.page}>
      <div className={styles.stack}>
        {/* Header with View Switcher */}
        <div className={styles.header}>
          <div>
            <div className={styles.headerTitleRow}>
              {viewInfo.icon}
              <Text size="xl" fw={500}>{viewInfo.title}</Text>
            </div>
            <Text c="dimmed" size="sm">{viewInfo.description}</Text>
          </div>

          {viewType === 'collections' && (
            <TextButton onClick={openCreateModal}>
              <IconPlus size={14} />
              New Collection
            </TextButton>
          )}

          {selectedNamespaceValue && viewType !== 'collections' && (
            <TextButton onClick={() => {
              setSelectedNamespaceValue(null);
              setFilteredImages([]);
            }}>
              <IconArrowLeft size={14} />
              Back to {viewInfo.title}
            </TextButton>
          )}
        </div>

        {/* View Type Switcher */}
        <SegmentedControl
          value={viewType}
          onChange={(value) => handleViewTypeChange(value as ViewType)}
          data={[
            { label: 'My Collections', value: 'collections' },
            { label: 'By Artist', value: 'artist' },
            { label: 'By Character', value: 'character' },
            { label: 'By Series', value: 'series' },
          ]}
        />

        {/* Search and Filter - only show for collections view or when viewing filtered images */}
        {(viewType === 'collections' || selectedNamespaceValue) && (
          <Grid>
            <Grid.Col span={{ base: 12, md: 6 }}>
              <TextInput
                placeholder={selectedNamespaceValue ? `Search ${selectedNamespaceValue} images...` : "Search collections..."}
                leftSection={<IconSearch size={16} />}
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.currentTarget.value)}
              />
            </Grid.Col>
            {viewType === 'collections' && (
              <Grid.Col span={{ base: 12, md: 6 }}>
                <TagsInput
                  placeholder="Filter by tags..."
                  value={selectedTags}
                  onChange={setSelectedTags}
                  data={allTags}
                  clearable
                />
              </Grid.Col>
            )}
          </Grid>
        )}

        {/* Main Content Area */}
        {renderMainContent()}

        {/* Collection Form Modal — shared for create & edit */}
        {(createModalOpened || editModalOpened) && (
          <Modal
            opened={createModalOpened || editModalOpened}
            onClose={() => {
              if (createModalOpened) closeCreateModal();
              if (editModalOpened) closeEditModal();
              resetForm();
            }}
            title={editModalOpened ? 'Edit Collection' : 'Create New Collection'}
            size="md"
            styles={glassModalStyles}
          >
            <div className={styles.modalBody}>
              <TextInput
                label="Name"
                placeholder="Enter collection name"
                value={formData.name}
                onChange={(event) => setFormData({ ...formData, name: event.currentTarget.value })}
                required
              />
              <TagsInput
                label="Tags"
                placeholder="Bulk apply tags to collection members"
                value={formData.tags}
                onChange={(tags) => setFormData({ ...formData, tags })}
                data={allTags}
              />
              <div className={styles.buttonRow}>
                <TextButton onClick={() => {
                  if (createModalOpened) closeCreateModal();
                  if (editModalOpened) closeEditModal();
                  resetForm();
                }}>
                  Cancel
                </TextButton>
                <TextButton onClick={editModalOpened ? handleEditCollection : handleCreateCollection}>
                  {editModalOpened ? 'Update' : 'Create'}
                </TextButton>
              </div>
            </div>
          </Modal>
        )}
      </div>
    </div>
  );
}
