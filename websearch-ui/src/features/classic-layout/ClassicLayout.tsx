import { HomePage } from '@/features/home'
import { LoadingPage } from '@/features/loading'
import { SearchPage } from '@/features/search'
import type { useSearch } from '@/hooks'
import { WindowManagerProvider, Taskbar, useWindowManager } from '@/components/WindowManager'
import { FloatingAdvancedToggle } from '@/components/composed'
import { WinBoxPortal, DevToolsScratchpad, AdminDashboard, StatsView, DebugPanel, InspectTool, ApiDocs, CollectionBrowser } from '@/components/DevTools'
import { useAdvancedMode } from '@/hooks'

interface ClassicLayoutProps {
  search: ReturnType<typeof useSearch>
}

function WindowRenderer() {
  const { windows, closeWindow } = useWindowManager()

  return (
    <>
      {windows.map(win => (
        <WinBoxPortal
          key={win.id}
          id={win.id}
          title={win.title}
          width={900}
          height={650}
          onClose={() => closeWindow(win.id)}
        >
          {win.type === 'devtools' && <DevToolsScratchpad />}
          {win.type === 'admin' && <AdminDashboard />}
          {win.type === 'stats' && <StatsView />}
          {win.type === 'debug' && <DebugPanel />}
          {win.type === 'inspect' && <InspectTool />}
          {win.type === 'apidocs' && <ApiDocs />}
          {win.type === 'browser' && <CollectionBrowser />}
        </WinBoxPortal>
      ))}
    </>
  )
}

export function ClassicLayout({ search }: ClassicLayoutProps) {
  const { isAdvancedMode } = useAdvancedMode()

  return (
    <WindowManagerProvider>
      <div className={`min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)] relative ${isAdvancedMode ? 'pb-9' : ''}`}>
        <FloatingAdvancedToggle />
        <WindowRenderer />
        {isAdvancedMode && <Taskbar />}
        {search.view === 'home' && <HomePage onSearch={search.search} />}

        {search.view === 'loading' && (
          <LoadingPage query={search.query} phase={search.phase} />
        )}

        {search.view === 'results' && (
          <SearchPage
            query={search.query}
            collections={search.collections}
            results={search.results}
            onNewSearch={search.search}
          />
        )}
      </div>
    </WindowManagerProvider>
  )
}
