import React from 'react';
import ReactDOM from 'react-dom/client';
import '../app/globals.css';
import { LibraryManager } from '../features/library/LibraryManager';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <LibraryManager />
  </React.StrictMode>,
);
