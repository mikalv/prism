import { useState, useEffect } from 'react'

export function DebugPanel() {
  const [logs, setLogs] = useState<string[]>([
    '[INFO] Application initialized',
    '[INFO] Theme loaded from storage',
  ])

  useEffect(() => {
    // A simple interceptor for console.log to show in debug panel could be added here
    const origLog = console.log
    console.log = (...args) => {
      setLogs(prev => [...prev, `[LOG] ${args.join(' ')}`])
      origLog(...args)
    }
    return () => {
      console.log = origLog
    }
  }, [])

  return (
    <div className="flex flex-col h-full w-full bg-[#1e1e2e] text-[#cdd6f4] p-4 font-mono text-sm">
      <h2 className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider mb-4 border-b border-[#313244] pb-2">Debug Panel</h2>
      
      <div className="flex-1 overflow-auto bg-[#181825] border border-[#313244] rounded p-2">
        {logs.map((log, i) => (
          <div key={i} className="mb-1 border-b border-[#313244]/50 pb-1 last:border-0">{log}</div>
        ))}
      </div>
    </div>
  )
}
