import { useWindowManager } from './WindowManagerContext'
import { Terminal, X } from 'lucide-react'

export function Taskbar() {
  const { windows, openWindow, closeWindow, toggleMinimize } = useWindowManager()

  return (
    <div className="fixed bottom-0 left-0 right-0 h-9 bg-[#1e1e2e] border-t border-[#313244] flex items-center px-1 z-[9999] font-sans text-xs">

      {/* Launcher Menu Buttons */}
      <div className="relative group flex items-center gap-1 border-r border-[#313244] pr-2 mr-2">
        <button
          onClick={() => openWindow('browser', 'Collection Browser')}
          className="flex items-center justify-center gap-1.5 h-7 px-2 bg-[#313244] hover:bg-[#45475a] text-[#89b4fa] font-bold rounded shadow-sm transition-colors border border-[#11111b]"
          title="Browse Collections"
        >
          <Terminal size={12} /> Browse
        </button>
        <button
          onClick={() => openWindow('admin', 'Admin Dashboard')}
          className="flex items-center justify-center gap-1.5 h-7 px-2 bg-[#313244] hover:bg-[#45475a] text-[#89b4fa] font-bold rounded shadow-sm transition-colors border border-[#11111b]"
          title="Admin Dashboard"
        >
          <Terminal size={12} /> Admin
        </button>
        <button
          onClick={() => openWindow('stats', 'System Stats')}
          className="flex items-center justify-center gap-1.5 h-7 px-2 bg-[#313244] hover:bg-[#45475a] text-[#89b4fa] font-bold rounded shadow-sm transition-colors border border-[#11111b]"
          title="System Stats"
        >
          <Terminal size={12} /> Stats
        </button>
        <button
          onClick={() => openWindow('debug', 'Debug Panel')}
          className="flex items-center justify-center gap-1.5 h-7 px-2 bg-[#313244] hover:bg-[#45475a] text-[#89b4fa] font-bold rounded shadow-sm transition-colors border border-[#11111b]"
          title="Debug Panel"
        >
          <Terminal size={12} /> Debug
        </button>
        <button
          onClick={() => openWindow('inspect', 'Inspect Tool')}
          className="flex items-center justify-center gap-1.5 h-7 px-2 bg-[#313244] hover:bg-[#45475a] text-[#89b4fa] font-bold rounded shadow-sm transition-colors border border-[#11111b]"
          title="Inspect Tool"
        >
          <Terminal size={12} /> Inspect
        </button>
        <button
          onClick={() => openWindow('devtools', 'API Console')}
          className="flex items-center justify-center gap-1.5 h-7 px-2 bg-[#313244] hover:bg-[#45475a] text-[#89b4fa] font-bold rounded shadow-sm transition-colors border border-[#11111b]"
          title="API Console"
        >
          <Terminal size={12} /> API Console
        </button>
        <button
          onClick={() => openWindow('apidocs', 'API Docs')}
          className="flex items-center justify-center gap-1.5 h-7 px-2 bg-[#313244] hover:bg-[#45475a] text-[#89b4fa] font-bold rounded shadow-sm transition-colors border border-[#11111b]"
          title="API Docs"
        >
          <Terminal size={12} /> API Docs
        </button>
      </div>

      {/* Window Tabs */}
      <div className="flex-1 flex items-center gap-1 overflow-x-auto h-full px-1">
        {windows.map(win => (
          <div
            key={win.id}
            onClick={() => toggleMinimize(win.id)}
            className={`flex items-center gap-2 h-7 px-3 min-w-[120px] max-w-[200px] border border-[#11111b] rounded cursor-pointer transition-colors shadow-sm select-none ${win.minimized
              ? 'bg-[#181825] text-[#a6adc8] hover:bg-[#313244]'
              : 'bg-[#4fc3f7] text-[#11111b] hover:bg-[#81d4fa] font-medium shadow-[inset_0_1px_rgba(255,255,255,0.4)]'
              }`}
          >
            <span className="truncate flex-1">{win.title}</span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                closeWindow(win.id);
              }}
              className={`hover:bg-black/20 rounded p-0.5 transition-colors ${win.minimized ? 'text-[#a6adc8]' : 'text-[#11111b]'}`}
            >
              <X size={12} />
            </button>
          </div>
        ))}
      </div>

    </div>
  )
}
