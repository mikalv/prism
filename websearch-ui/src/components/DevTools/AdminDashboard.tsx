import { useState } from 'react'

export function AdminDashboard() {
  const [isIndexing, setIsIndexing] = useState(true)

  return (
    <div className="flex flex-col h-full w-full bg-[#1e1e2e] text-[#cdd6f4] p-4 font-mono text-sm">
      <h2 className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider mb-4 border-b border-[#313244] pb-2">Admin Dashboard</h2>
      
      <div className="flex flex-col gap-4">
        <div className="bg-[#181825] p-4 rounded border border-[#313244] flex items-center justify-between">
          <div>
            <div className="font-semibold text-white">Indexing Engine</div>
            <div className="text-xs text-[#a6adc8]">Toggle the backend indexing process.</div>
          </div>
          <button 
            onClick={() => setIsIndexing(!isIndexing)}
            className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
              isIndexing ? 'bg-green-900/40 text-green-400 hover:bg-green-900/60' : 'bg-red-900/40 text-red-400 hover:bg-red-900/60'
            }`}
          >
            {isIndexing ? 'Active' : 'Paused'}
          </button>
        </div>
        
        <div className="bg-[#181825] p-4 rounded border border-[#313244] flex items-center justify-between">
          <div>
            <div className="font-semibold text-white">Clear Cache</div>
            <div className="text-xs text-[#a6adc8]">Clear local application cache and stored state.</div>
          </div>
          <button 
            onClick={() => {
              localStorage.clear();
              window.location.reload();
            }}
            className="px-3 py-1 bg-yellow-900/40 text-yellow-400 hover:bg-yellow-900/60 rounded text-xs font-medium transition-colors"
          >
            Clear Cache
          </button>
        </div>
      </div>
    </div>
  )
}
