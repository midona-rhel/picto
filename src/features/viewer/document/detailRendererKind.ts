export type DetailRendererKind =
  | 'image' | 'jpeg-xl' | 'video' | 'audio' | 'pdf' | 'flash' | 'font'
  | 'text-document' | 'docx' | 'pptx' | 'epub' | 'cbz' | 'djvu' | 'unsupported';

export function detailRendererKind(mimeType: string): DetailRendererKind {
  if (mimeType.startsWith('video/')) return 'video';
  if (mimeType.startsWith('audio/')) return 'audio';
  if (mimeType === 'image/jxl') return 'jpeg-xl';
  if (mimeType === 'application/pdf') return 'pdf';
  if (mimeType === 'application/vnd.openxmlformats-officedocument.wordprocessingml.document') return 'docx';
  if (mimeType === 'application/vnd.openxmlformats-officedocument.presentationml.presentation') return 'pptx';
  if (mimeType === 'application/epub+zip') return 'epub';
  if (mimeType === 'application/vnd.comicbook+zip' || mimeType === 'application/x-cbz') return 'cbz';
  if (mimeType === 'image/vnd.djvu' || mimeType === 'image/x-djvu') return 'djvu';
  if (mimeType === 'text/plain' || mimeType === 'text/markdown' || mimeType === 'application/json' || mimeType === 'application/rtf' || mimeType === 'text/rtf') return 'text-document';
  if (mimeType === 'application/x-shockwave-flash') return 'flash';
  if (mimeType.startsWith('font/') || mimeType === 'application/font-sfnt') return 'font';
  if (mimeType.startsWith('image/')) return 'image';
  return 'unsupported';
}
