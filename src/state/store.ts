/**
 * Default Jotai store instance.
 *
 * Use this for imperative access outside React components (controllers,
 * event handlers, runtime appliers). Components use useAtom/useAtomValue
 * which read from this store via the Provider.
 */

import { createStore } from 'jotai';

export const store = createStore();
