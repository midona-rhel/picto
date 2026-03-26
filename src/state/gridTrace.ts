import { atom } from 'jotai';
import type { GridFrameTraceStoreSnapshot } from '../features/grid/canvas/gridTrace';

export const gridFrameTraceAtom = atom<GridFrameTraceStoreSnapshot | null>(null);
