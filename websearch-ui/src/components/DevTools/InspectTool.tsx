import { useState } from 'react'
import { Search } from 'lucide-react'

export function InspectTool() {
  const [docId, setDocId] = useState('')
  const [docData, setDocData] = useState<any>(null)

  const handleInspect = () => {
    if (!docId.trim()) return
    // Mock inspection
    setDocData({
      id: docId,
      _source: {
        title: "Mock Document Title",
        content: "This is a mock representation of the document data for inspection.",
        timestamp: new Date().toISOString()
      },
      _score: 1.0
    })
  }

  return (
    <div className="flex flex-col h-full w-full bg-[#1e1e2e] text-[#cdd6f4] p-4 font-mono text-sm">
      <h2 className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider mb-4 border-b border-[#313244] pb-2">Inspect Document</h2>
      
      <div className="flex gap-2 mb-4">
        <input 
          type="text" 
          value={docId}
          onChange={e => setDocId(e.target.value)}
          placeholder="Enter Document ID"
          className="flex-1 bg-[#181825] border border-[#313244] rounded px-3 py-1 text-[#cdd6f4] focus:outline-none focus:border-[#89b4fa]"
        />
        <button 
          onClick={handleInspect}
          className="flex items-center gap-1.5 px-3 py-1 bg-[#313244] hover:bg-[#45475a] text-[#89b4fa] font-bold rounded shadow-sm transition-colors border border-[#11111b]"
        >
          <Search size={14} /> Inspect
        </button>
      </div>

      <div className="flex-1 overflow-auto bg-[#181825] border border-[#313244] rounded p-4">
        {docData ? (
          <pre className="whitespace-pre-wrap text-green-400">{JSON.stringify(docData, null, 2)}</pre>
        ) : (
          <div className="text-[#a6adc8] italic">Enter a document ID to inspect its raw data...</div>
        )}
      </div>
    </div>
  )
}
