export class GridTransitionCoordinator {
  private delayId: number | null = null;
  private frameId: number | null = null;

  scheduleDelay(callback: () => void, delayMs: number): void {
    this.cancelDelay();
    this.delayId = window.setTimeout(() => {
      this.delayId = null;
      callback();
    }, delayMs);
  }

  scheduleFrame(callback: () => void): void {
    this.cancelFrame();
    this.frameId = window.requestAnimationFrame(() => {
      this.frameId = null;
      callback();
    });
  }

  cancelDelay(): void {
    if (this.delayId == null) return;
    window.clearTimeout(this.delayId);
    this.delayId = null;
  }

  cancelFrame(): void {
    if (this.frameId == null) return;
    window.cancelAnimationFrame(this.frameId);
    this.frameId = null;
  }

  cancel(): void {
    this.cancelDelay();
    this.cancelFrame();
  }
}
