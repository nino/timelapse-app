import { GlobalRegistrator } from '@happy-dom/global-registrator';

// Gives `bun test` a browser-like `window`/`document` for React Testing Library.
GlobalRegistrator.register();
