import { useReducer } from 'react';
import { gridRuntimeReducer, type GridRuntimeAction } from './gridRuntimeReducer';
import {
  createInitialState,
  type GridRuntimeState,
  type GridRuntimeInitProps,
} from './gridRuntimeState';

export function useGridRuntime(props: GridRuntimeInitProps): {
  state: GridRuntimeState;
  dispatch: React.Dispatch<GridRuntimeAction>;
} {
  const [state, dispatch] = useReducer(gridRuntimeReducer, props, createInitialState);
  return { state, dispatch };
}
