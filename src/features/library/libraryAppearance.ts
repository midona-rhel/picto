export async function setCurrentLibraryImageIcon(hash: string): Promise<void> {
  const library = (window as any).picto?.library;
  if (!library) throw new Error('Library service is unavailable.');
  const config = await library.getConfig();
  if (!config.currentPath) throw new Error('No library is open.');
  await library.setMeta(config.currentPath, { imageHash: hash, icon: null });
}
