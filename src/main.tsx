import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource/space-grotesk/500.css';
import '@fontsource/space-grotesk/600.css';
import '@fontsource/space-grotesk/700.css';
import './tokens.css';
import './styles.css';
import App from './App';
import { ErrorBoundary } from './ErrorBoundary';

const root = createRoot(document.getElementById('root')!);
if (import.meta.env.DEV && location.pathname.startsWith('/comps/')) {
  void import('./comps/Comps').then(({ mountComp }) => mountComp(root, location.pathname.slice('/comps/'.length)));
} else {
  root.render(
    <StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </StrictMode>,
  );
}
