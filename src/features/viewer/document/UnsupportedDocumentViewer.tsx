import { DocumentViewerShell } from './DocumentViewerShell';

interface Props { mimeType: string }

export function UnsupportedDocumentViewer({ mimeType }: Props) {
  return (
    <DocumentViewerShell
      error={`Preview is not available for ${mimeType || 'this file type'}.`}
      pageNumber={1}
      pageCount={1}
      navigationLabel="document"
    />
  );
}
