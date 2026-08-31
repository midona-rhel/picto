import { DocumentViewerShell } from './DocumentViewerShell';
import { t } from '../../../i18n';

interface Props { mimeType: string }

export function UnsupportedDocumentViewer({ mimeType }: Props) {
  return (
    <DocumentViewerShell
      error={t('Preview is not available for {value0}.', { value0: mimeType || t('this file type') })}
      pageNumber={1}
      pageCount={1}
      navigationLabel={t('document')}
    />
  );
}
