import { atom } from 'jotai';

/** Number of permanent-delete operations awaiting backend settlement. */
export const permanentDeletesInFlightAtom = atom(0);
