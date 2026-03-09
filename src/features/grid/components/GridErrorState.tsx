import { StateActions, StateBlock } from '../../../shared/components/state';
import { TextButton } from '../../../shared/components/TextButton';

export function GridErrorState(props: {
  error: string;
  onRetry: () => void;
}) {
  const { error, onRetry } = props;

  return (
    <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
      <StateBlock
        variant="error"
        title="Failed to load images"
        description={error}
        action={(
          <StateActions>
            <TextButton onClick={onRetry}>Retry</TextButton>
          </StateActions>
        )}
      />
    </div>
  );
}
