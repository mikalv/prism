import { useState, useEffect } from 'react'

export function DebugPanel() {
  const [logs, setLogs] = useState<string[]>([
    '[INFO] Application initialized',
    '[INFO] Theme loaded from storage',
  ])
  const [serverDebug, setServerDebug] = useState<any>(null)

  useEffect(() => {
    // Intercept console.log
    const origLog = console.log
    console.log = (...args) => {
      setLogs(prev => [...prev, `[LOG] ${args.join(' ')}`])
      origLog(...args)
    }
    
    // Fetch server debug info
    const fetchServerDebug = async () => {
      try {
        const API_BASE_URL = import.meta.env.VITE_API_URL || ''
        const res = await fetch(`${API_BASE_URL}/admin/debug`)
        if (res.ok) setServerDebug(await res.json())
      } catch (err) {
        setLogs(prev => [...prev, `[ERROR] Failed to fetch server debug: ${String(err)}`])
      }
    }
    fetchServerDebug()
    
    return () => {
      console.log = origLog
    }
  }, [])

  return (
    <div className="flex flex-col h-full w-full bg-[#1e1e2e] text-[#cdd6f4] p-4 font-mono text-sm overflow-auto">
      <h2 className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider mb-4 border-b border-[#313244] pb-2">Debug Panel</h2>
      
      <div className="flex flex-col md:flex-row gap-4 flex-1">
        <div className="flex-1 flex flex-col min-h-[200px]">
          <div className="text-xs text-[#a6adc8] mb-2 font-semibold">Client Logs</div>
          <div className="flex-1 overflow-auto bg-[#181825] border border-[#313244] rounded p-2 text-xs">
            {logs.map((log, i) => (
              <div key={i} className="mb-1 border-b border-[#313244]/50 pb-1 last:border-0 break-words">{log}</div>
            ))}
          </div>
        </div>
        
        <div className="flex-1 flex flex-col min-h-[200px]">
          <div className="text-xs text-[#a6adc8] mb-2 font-semibold">Server /admin/debug</div>
          <pre className="flex-1 overflow-auto bg-[#181825] border border-[#313244] rounded p-2 text-xs text-yellow-400">
            {serverDebug ? JSON.stringify(serverDebug, null, 2) : 'Loading server debug info...'}
          </pre>
        </div>
      </div>
    </div>
  )
}
