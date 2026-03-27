import { useEffect } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { gridRendererAtom } from '../../../state/grid';
import { gridPerfAtom } from '../../../state/gridPerf';
import { gridFrameTraceAtom } from '../../../state/gridTrace';
import { DomGridFallback, type CanvasGridProps } from './DomGridFallback';
import { GridSceneHost } from '../webgl/GridSceneHost';

export function CanvasGrid(props: CanvasGridProps) {
  const renderer = useAtomValue(gridRendererAtom);
  const setGridPerf = useSetAtom(gridPerfAtom);
  const setGridFrameTrace = useSetAtom(gridFrameTraceAtom);

  useEffect(() => {
    setGridPerf(null);
    setGridFrameTrace(null);
    return () => {
      setGridPerf(null);
      setGridFrameTrace(null);
    };
  }, [setGridFrameTrace, setGridPerf]);

  return renderer === 'canvas'
    ? <DomGridFallback {...props} />
    : <GridSceneHost {...props} />;
}

export type { CanvasGridProps } from './DomGridFallback';
