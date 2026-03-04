import React from 'react';
import { Alert, Text } from '@mantine/core';
import { TextButton } from './ui/TextButton';

interface AppErrorBoundaryState {
  hasError: boolean;
  errorMessage?: string;
}

export class AppErrorBoundary extends React.Component<React.PropsWithChildren, AppErrorBoundaryState> {
  state: AppErrorBoundaryState = { hasError: false };

  static getDerivedStateFromError(error: Error): AppErrorBoundaryState {
    return { hasError: true, errorMessage: error.message };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('Unhandled UI error:', error, errorInfo);
  }

  private handleReload = () => {
    window.location.reload();
  };

  render() {
    if (!this.state.hasError) {
      return this.props.children;
    }

    return (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: '100vw', height: '100vh' }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16, width: 520 }}>
          <Alert color="red" title="Application Error">
            A runtime UI error occurred. You can reload and continue working.
          </Alert>
          {this.state.errorMessage && (
            <Text size="sm" c="dimmed">
              {this.state.errorMessage}
            </Text>
          )}
          <TextButton onClick={this.handleReload}>
            Reload App
          </TextButton>
        </div>
      </div>
    );
  }
}
