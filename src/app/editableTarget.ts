export function isEditableTarget(target: EventTarget | null): boolean {
  if (target instanceof HTMLInputElement
    || target instanceof HTMLTextAreaElement
    || target instanceof HTMLSelectElement) {
    return true;
  }
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable === true
    || target.closest('[contenteditable]:not([contenteditable="false"])') !== null;
}
