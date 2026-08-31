export async function claimAssociatedPictoPack(): Promise<string | null> {
  return (window as any).picto.associatedFiles.claimPictoPack();
}

