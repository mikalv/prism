import { useState, useEffect } from 'react'

export function StatsView() {
  const [stats, setStats] = useState<any>(null)
  const [cacheStats, setCacheStats] = useState<any>(null)
  const [error, setError] = useState('')
  const [loadHistory, setLoadHistory] = useState<number[]>(Array(20).fill(0))

  useEffect(() => {
    const API_BASE_URL = import.meta.env.VITE_API_URL || ''
    
    const fetchStats = async () => {
      try {
        const [serverRes, cacheRes, loadRes] = await Promise.all([
          fetch(`${API_BASE_URL}/stats/server`),
          fetch(`${API_BASE_URL}/stats/cache`),
          fetch(`${API_BASE_URL}/stats/load`)
        ])
        if (serverRes.ok) setStats(await serverRes.json())
        if (cacheRes.ok) setCacheStats(await cacheRes.json())
        
        if (loadRes.ok) {
          const loadData = await loadRes.json()
          setLoadHistory(prev => {
            const next = [...prev.slice(1), loadData.cpu_usage_percent]
            return next
          })
        }
      } catch (err: any) {
        setError(err.message)
      }
    }
    
    fetchStats()
    const interval = setInterval(fetchStats, 5000)
    return () => clearInterval(interval)
  }, [])

  return (
    <div className="flex flex-col h-full w-full bg-[#1e1e2e] text-[#cdd6f4] p-4 font-mono text-sm overflow-auto">
      <h2 className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider mb-4 border-b border-[#313244] pb-2">System Stats & Load</h2>
      
      {error && <div className="text-red-400 mb-4">{error}</div>}
      
      <div className="mb-4 bg-[#181825] p-4 rounded border border-[#313244]">
        <div className="text-xs text-[#a6adc8] mb-2 font-semibold border-b border-[#313244] pb-1">Server Load (Simulated)</div>
        <div className="flex items-end h-24 gap-1 pt-2">
          {loadHistory.map((val, i) => (
            <div 
              key={i} 
              className="flex-1 bg-[var(--accent)]/50 hover:bg-[var(--accent)] transition-all rounded-t-sm"
              style={{ height: `${val}%`, minHeight: '4px' }}
              title={`${val}% load`}
            />
          ))}
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="bg-[#181825] p-4 rounded border border-[#313244]">
          <div className="text-xs text-[#a6adc8] mb-2 font-semibold border-b border-[#313244] pb-1">Server Stats</div>
          <pre className="text-xs overflow-auto text-green-400 h-48">
            {stats ? JSON.stringify(stats, null, 2) : 'Loading...'}
          </pre>
        </div>
        <div className="bg-[#181825] p-4 rounded border border-[#313244]">
          <div className="text-xs text-[#a6adc8] mb-2 font-semibold border-b border-[#313244] pb-1">Cache Stats</div>
          <pre className="text-xs overflow-auto text-blue-400 h-48">
            {cacheStats ? JSON.stringify(cacheStats, null, 2) : 'Loading...'}
          </pre>
        </div>
      </div>
    </div>
  )
}
