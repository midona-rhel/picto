import { useCallback, useMemo, useReducer } from 'react';
import { type GridRuntimeAction } from './gridRuntimeReducer';
import { gridDataReducer } from './gridDataReducer';
import { gridUiReducer } from './gridUiReducer';
import {
  type GridRuntimeState,
  type GridRuntimeInitProps,
} from './gridRuntimeState';
import { createInitialGridDataState } from './gridDataState';
import { createInitialGridUiState } from './gridUiState';

export function useGridRuntime(props: GridRuntimeInitProps): {
  state: GridRuntimeState;
  dispatch: React.Dispatch<GridRuntimeAction>;
} {
  const [dataState, dispatchData] = useReducer(gridDataReducer, props, createInitialGridDataState);
  const [uiState, dispatchUi] = useReducer(gridUiReducer, undefined, createInitialGridUiState);

  const dispatch = useCallback((action: GridRuntimeAction) => {
    dispatchData(action);
    dispatchUi(action);
  }, []);

  const state = useMemo(() => ({
    ...dataState,
    ...uiState,
  }), [dataState, uiState]);

  return { state, dispatch };
}
