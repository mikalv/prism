import { useState } from 'react'
import { Play } from 'lucide-react'

const DEFAULT_QUERY = `POST /api/search
{
  "query": "test",
  "limit": 10
}`

export function DevToolsScratchpad() {
  const [requestText, setRequestText] = useState(DEFAULT_QUERY)
  const [responseText, setResponseText] = useState('// Responses will appear here...')
  const [isRunning, setIsRunning] = useState(false)

  const handleRun = async () => {
    if (!requestText.trim()) return

    setIsRunning(true)
    const API_BASE_URL = import.meta.env.VITE_API_URL || ''

    try {
      const lines = requestText.trim().split('\n')
      const firstLine = lines[0].trim().split(' ')
      if (firstLine.length < 2) {
        throw new Error('Invalid request format. Expected: METHOD /path')
      }

      const method = firstLine[0].toUpperCase()
      const path = firstLine[1]
      const bodyText = lines.slice(1).join('\n').trim()

      const options: RequestInit = {
        method,
        headers: {
          'Content-Type': 'application/json'
        }
      }

      if (['POST', 'PUT', 'PATCH'].includes(method) && bodyText) {
        // Validate JSON before sending
        JSON.parse(bodyText)
        options.body = bodyText
      }

      const res = await fetch(`${API_BASE_URL}${path}`, options)

      const contentType = res.headers.get('content-type')
      let data
      if (contentType && contentType.includes('application/json')) {
        data = await res.json()
      } else {
        data = await res.text()
      }

      setResponseText(
        `// Status: ${res.status} ${res.statusText}\n` +
        JSON.stringify(data, null, 2)
      )
    } catch (err: any) {
      setResponseText(`// Error\n${err.message || String(err)}`)
    } finally {
      setIsRunning(false)
    }
  }

  return (
    <div className="flex h-full w-full bg-[#1e1e2e] text-[#cdd6f4] font-mono text-sm">
      {/* Left Pane: Editor */}
      <div className="flex flex-col w-1/2 border-r border-[#313244]">
        <div className="flex items-center justify-between p-2 bg-[#181825] border-b border-[#313244]">
          <span className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider">Console</span>
          <button
            onClick={handleRun}
            disabled={isRunning}
            className="flex items-center gap-1.5 px-3 py-1 bg-green-900/40 text-green-400 hover:bg-green-900/60 rounded text-xs font-medium transition-colors disabled:opacity-50"
          >
            <Play size={14} />
            {isRunning ? 'Running...' : 'Run'}
          </button>
        </div>
        <textarea
          className="flex-1 w-full bg-transparent p-4 resize-none outline-none focus:ring-inset focus:ring-1 focus:ring-[#89b4fa] transition-shadow text-[#cdd6f4] font-mono"
          value={requestText}
          onChange={e => setRequestText(e.target.value)}
          spellCheck={false}
        />
      </div>

      {/* Right Pane: Output */}
      <div className="flex flex-col w-1/2">
        <div className="p-2 bg-[#181825] border-b border-[#313244]">
          <span className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider">Response</span>
        </div>
        <div className="flex-1 p-4 overflow-auto bg-[#1e1e2e]">
          <pre className="whitespace-pre-wrap word-break text-[#a6adc8]">{responseText}</pre>
        </div>
      </div>
    </div>
  )
}
