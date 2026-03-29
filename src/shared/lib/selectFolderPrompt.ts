import type { SidebarNodeDto } from '../types/canonical';

export function promptForFolderId(nodes: SidebarNodeDto[]): number | null {
  const folderNodes = nodes
    .filter((node) => node.kind === 'folder')
    .sort((a, b) => a.name.localeCompare(b.name));
  if (folderNodes.length === 0) return null;

  const promptLines = folderNodes
    .slice(0, 20)
    .map((node) => {
      const parsed = Number.parseInt(node.id.replace('folder:', ''), 10);
      return `${parsed}: ${node.name}`;
    });
  const input = window.prompt(
    `Add to folder. Enter folder id or exact name.\n\n${promptLines.join('\n')}`,
    '',
  );
  if (!input) return null;
  const trimmed = input.trim();
  const asId = Number.parseInt(trimmed, 10);
  if (!Number.isNaN(asId)) return asId;
  const match = folderNodes.find((node) => node.name.toLowerCase() === trimmed.toLowerCase());
  if (!match) return null;
  const parsed = Number.parseInt(match.id.replace('folder:', ''), 10);
  return Number.isNaN(parsed) ? null : parsed;
}
