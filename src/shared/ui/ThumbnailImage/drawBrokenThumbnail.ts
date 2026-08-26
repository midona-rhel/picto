import {
  BROKEN_DOCUMENT_BODY_PATH,
  BROKEN_DOCUMENT_CRACK_PATH,
  BROKEN_DOCUMENT_FOLD_PATH,
} from './brokenThumbnailGeometry';

/** Canvas counterpart to BrokenThumbnail, shared by the grid and drag image. */
export function drawBrokenThumbnail(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  background: string,
): void {
  const markHeight = Math.min(height * 0.64, 176);
  const markWidth = markHeight * (160 / 176);

  context.save();
  context.translate(x + (width - markWidth) / 2, y + (height - markHeight) / 2);
  context.scale(markWidth / 160, markHeight / 176);
  context.shadowColor = 'rgba(0,0,0,.36)';
  context.shadowBlur = 9;
  context.shadowOffsetY = 6;
  const paper = context.createLinearGradient(45, 18, 112, 160);
  paper.addColorStop(0, 'rgba(247,248,250,.9)');
  paper.addColorStop(1, 'rgba(167,171,178,.66)');
  context.fillStyle = paper;
  context.fill(new Path2D(BROKEN_DOCUMENT_BODY_PATH));
  context.shadowColor = 'transparent';
  const fold = context.createLinearGradient(108, 18, 137, 47);
  fold.addColorStop(0, 'rgba(236,238,241,.86)');
  fold.addColorStop(1, 'rgba(157,162,170,.62)');
  context.fillStyle = fold;
  context.fill(new Path2D('M108 18L137 47H108Z'));
  context.strokeStyle = background;
  context.lineWidth = 11;
  context.lineJoin = 'miter';
  context.stroke(new Path2D(BROKEN_DOCUMENT_FOLD_PATH));
  context.lineCap = 'butt';
  context.stroke(new Path2D(BROKEN_DOCUMENT_CRACK_PATH));
  context.restore();
}
