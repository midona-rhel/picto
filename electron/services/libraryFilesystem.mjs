export async function isFailedCloudJoinDirectory(fs, targetRoot) {
  const entries = await fs.readdir(targetRoot, { withFileTypes: true }).catch(() => null);
  if (!entries || entries.length === 0) return false;
  return entries.every((entry) =>
    (entry.isDirectory() && entry.name === 'blobs')
    || (entry.isFile() && /^failed-cloud-join-[\w-]+\.sqlite$/.test(entry.name)),
  );
}

export function isInsideLibraryPackage(pathApi, candidatePath) {
  let current = pathApi.resolve(candidatePath);
  while (true) {
    if (pathApi.basename(current).toLowerCase().endsWith('.library')) return true;
    const parent = pathApi.dirname(current);
    if (parent === current) return false;
    current = parent;
  }
}

async function verifyDirectoryCopy(fs, pathApi, source, destination) {
  let sourceCount = 0;
  // Stream one directory at a time; library-sized manifests can exhaust the main-process heap.
  for await (const entry of await fs.opendir(source)) {
    sourceCount += 1;
    const original = pathApi.join(source, entry.name);
    const copied = pathApi.join(destination, entry.name);
    const copiedStat = await fs.lstat(copied);
    let matches = false;
    if (entry.isDirectory()) {
      matches = copiedStat.isDirectory();
      if (matches) await verifyDirectoryCopy(fs, pathApi, original, copied);
    } else if (entry.isFile()) {
      matches = copiedStat.isFile()
        && (await fs.stat(original)).size === copiedStat.size
        && await filesEqual(fs, original, copied);
    } else if (entry.isSymbolicLink()) {
      matches = copiedStat.isSymbolicLink()
        && await fs.readlink(original) === await fs.readlink(copied);
    }
    if (!matches) throw new Error('Copied library verification failed');
  }
  let destinationCount = 0;
  for await (const _entry of await fs.opendir(destination)) destinationCount += 1;
  if (destinationCount !== sourceCount) {
    throw new Error('Copied library verification failed');
  }
}

async function filesEqual(fs, source, destination) {
  const sourceHandle = await fs.open(source, 'r');
  let destinationHandle;
  try {
    destinationHandle = await fs.open(destination, 'r');
    const sourceBuffer = Buffer.allocUnsafe(1024 * 1024);
    const destinationBuffer = Buffer.allocUnsafe(sourceBuffer.length);
    let position = 0;
    while (true) {
      const [sourceRead, destinationRead] = await Promise.all([
        sourceHandle.read(sourceBuffer, 0, sourceBuffer.length, position),
        destinationHandle.read(destinationBuffer, 0, destinationBuffer.length, position),
      ]);
      if (sourceRead.bytesRead !== destinationRead.bytesRead) return false;
      if (sourceRead.bytesRead === 0) return true;
      if (!sourceBuffer.subarray(0, sourceRead.bytesRead)
        .equals(destinationBuffer.subarray(0, destinationRead.bytesRead))) return false;
      position += sourceRead.bytesRead;
    }
  } finally {
    await Promise.all([
      sourceHandle.close(),
      destinationHandle?.close(),
    ]);
  }
}

export function createLibraryFilesystem({ fs, path }) {
  let relocationSequence = 0;

  async function isValidLibrary(libraryPath) {
    try {
      await fs.access(path.join(libraryPath, 'library.sqlite'));
      return true;
    } catch {
      return false;
    }
  }

  async function ownedLibraryPath(libraryPath) {
    try {
      const entry = await fs.lstat(libraryPath);
      if (!entry.isDirectory() || entry.isSymbolicLink()) throw new Error('not a real directory');
      const resolved = await fs.realpath(libraryPath);
      if (!path.basename(resolved).toLowerCase().endsWith('.library')) {
        throw new Error('not a library package');
      }
      const database = await fs.lstat(path.join(resolved, 'library.sqlite'));
      if (!database.isFile()) throw new Error('missing library database');
      return resolved;
    } catch {
      throw new Error('Refusing to modify a path that is no longer a Picto library.');
    }
  }

  async function moveLibraryDirectory(oldPath, newPath) {
    try {
      await fs.rename(oldPath, newPath);
      return;
    } catch (error) {
      if (error?.code !== 'EXDEV') throw error;
    }

    const stagingPath = path.join(
      path.dirname(newPath),
      `.${path.basename(newPath)}.picto-moving-${process.pid}-${++relocationSequence}`,
    );
    let published = false;
    try {
      await fs.cp(oldPath, stagingPath, {
        recursive: true,
        force: false,
        errorOnExist: true,
        preserveTimestamps: true,
        verbatimSymlinks: true,
      });
      await verifyDirectoryCopy(fs, path, oldPath, stagingPath);
      const destinationExists = await fs.access(newPath).then(() => true, (error) => {
        if (error?.code === 'ENOENT') return false;
        throw error;
      });
      if (destinationExists) throw new Error('The move destination appeared while copying the library');
      await fs.rename(stagingPath, newPath);
      published = true;
    } finally {
      if (!published) await fs.rm(stagingPath, { recursive: true, force: true }).catch(() => {});
    }

    try {
      await fs.rm(oldPath, { recursive: true, force: false });
    } catch (error) {
      console.warn('[library] unable to remove the original after a verified cross-volume copy', {
        oldPath,
        newPath,
        message: error?.message ?? String(error),
      });
    }
  }

  return { isValidLibrary, ownedLibraryPath, moveLibraryDirectory };
}
