import { useGridTransitionController } from './useGridTransitionController';

export function useGridScopeTransition(
  args: Parameters<typeof useGridTransitionController>[0],
): ReturnType<typeof useGridTransitionController> {
  return useGridTransitionController(args);
}
