# Rove WebUI

Modern, multi-tab web interface for Rove AI Agent built with Next.js.

## Quick Start

### Install Dependencies

```bash
cd /Users/as/workspace/rove/core/webui
npm install
```

### Development Mode

```bash
# Start Next.js dev server (port 3000)
npm run dev

# Open browser to http://localhost:3000
```

### Build for Production

```bash
# Build static export
npm run build

# Output is in ./dist folder
```

### Serve with Rove Daemon

The Rove daemon automatically serves the built WebUI:

```bash
# After building, start the daemon
cd /Users/as/workspace/rove/core/target/debug
ROVE_OPENAI_API_KEY=test ./rove daemon --port 47630

# Open the hosted shell at https://app.roveai.co
```

Or use the build script:
```bash
cd /Users/as/workspace/rove/core
./build-webui.sh
```

## Project Structure

```
webui-next/
├── src/
│   ├── app/                    # Next.js App Router
│   │   ├── globals.css         # Global styles
│   │   ├── layout.tsx          # Root layout
│   │   ├── page.tsx            # Messages tab (home)
│   │   ├── config/page.tsx     # Config tab
│   │   ├── settings/page.tsx   # Settings tab
│   │   └── plugins/page.tsx    # Plugins tab
│   ├── components/
│   │   └── Nav.tsx             # Navigation tabs
│   ├── stores/
│   │   └── roveStore.ts        # Zustand state management
│   └── lib/                    # Utilities
├── package.json
├── tailwind.config.ts
├── tsconfig.json
└── next.config.js
```

## Tabs

### Messages (`/`)
- Task input with Ctrl+Enter shortcut
- Real-time task status updates
- Task history with filtering
- Connection status indicator

### Config (`/config`)
- WebSocket connection status
- LLM provider status
- Memory system settings
- Gateway configuration

### Settings (`/settings`)
- Appearance options
- Notification preferences
- Privacy settings
- Data management

### Plugins (`/plugins`)
- Core plugins management
- WASM plugins (future)
- MCP servers (future)

## State Management

Uses [Zustand](https://github.com/pmndrs/zustand) for lightweight state:

```typescript
import { useRoveStore } from '@/stores/roveStore';

function MyComponent() {
  const { tasks, ws, sendTask } = useRoveStore();
  // ...
}
```

## WebSocket Protocol

Same as vanilla JS version:

**Client → Server:**
```json
{ "type": "start_task", "input": "Do something" }
{ "type": "ping" }
```

**Server → Client:**
```json
{ "type": "connected", "version": "0.0.3" }
{ "type": "accepted", "task_id": "uuid" }
{ "type": "progress", "message": "Running..." }
{ "type": "result", "answer": "...", "duration_ms": 1234 }
{ "type": "error", "message": "..." }
```

## Adding New Tabs

1. Create new route: `src/app/my-tab/page.tsx`
2. Add to Nav component
3. Implement your UI

Example:
```tsx
// src/app/my-tab/page.tsx
import Nav from '@/components/Nav';

export default function MyTabPage() {
  return (
    <div>
      <Nav />
      <h1>My Tab</h1>
      {/* Your content */}
    </div>
  );
}
```

## Styling

Uses Tailwind CSS with custom color variables:

```css
--background: #0f0f0f
--surface: #1a1a1a
--surface2: #252525
--primary: #3b82f6
--success: #10b981
--error: #ef4444
--warning: #f59e0b
```

## Build Configuration

`next.config.js`:
```javascript
{
  output: 'export',      // Static export
  distDir: 'dist',       // Output to ./dist
  images: { unoptimized: true } // No image optimization needed
}
```

## Troubleshooting

### Build fails
```bash
# Clear cache
rm -rf .next node_modules
npm install
npm run build
```

### WebSocket not connecting
- Check daemon is running: `rove status`
- Check correct port: default is 47630
- Check browser console for errors

### Styles not loading
- Ensure Tailwind is built: `npm run build`
- Check `globals.css` is imported in `layout.tsx`

## License

MIT — Same as Rove
