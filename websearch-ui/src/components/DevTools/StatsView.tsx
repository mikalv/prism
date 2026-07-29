import { useState, useEffect } from 'react'

export function StatsView() {
  const [stats, setStats] = useState({
    totalQueries: 0,
    averageLatency: 0,
    activeUsers: 1,
    indexedDocuments: 0,
  })

  // Mock fetching stats
  useEffect(() => {
    const interval = setInterval(() => {
      setStats(prev => ({
        ...prev,
        totalQueries: prev.totalQueries + Math.floor(Math.random() * 5),
        averageLatency: 45 + Math.floor(Math.random() * 20),
        indexedDocuments: 145020 + Math.floor(Math.random() * 10),
      }))
    }, 2000)
    return () => clearInterval(interval)
  }, [])

  return (
    <div className="flex flex-col h-full w-full bg-[#1e1e2e] text-[#cdd6f4] p-4 font-mono text-sm">
      <h2 className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider mb-4 border-b border-[#313244] pb-2">System Stats</h2>
      
      <div className="grid grid-cols-2 gap-4">
        <div className="bg-[#181825] p-4 rounded border border-[#313244]">
          <div className="text-xs text-[#a6adc8] mb-1">Total Queries</div>
          <div className="text-2xl font-semibold text-white">{stats.totalQueries.toLocaleString()}</div>
        </div>
        <div className="bg-[#181825] p-4 rounded border border-[#313244]">
          <div className="text-xs text-[#a6adc8] mb-1">Avg Latency</div>
          <div className="text-2xl font-semibold text-blue-400">{stats.averageLatency}ms</div>
        </div>
        <div className="bg-[#181825] p-4 rounded border border-[#313244]">
          <div className="text-xs text-[#a6adc8] mb-1">Indexed Documents</div>
          <div className="text-2xl font-semibold text-white">{stats.indexedDocuments.toLocaleString()}</div>
        </div>
        <div className="bg-[#181825] p-4 rounded border border-[#313244]">
          <div className="text-xs text-[#a6adc8] mb-1">Active Users</div>
          <div className="text-2xl font-semibold text-white">{stats.activeUsers}</div>
        </div>
      </div>
    </div>
  )
}
