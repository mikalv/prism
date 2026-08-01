import { useState } from 'react'
import { ExternalLink, RefreshCw } from 'lucide-react'

export function ApiDocs() {
  const defaultDocsUrl = 'https://mikalv.github.io/prism/reference/api-reference/'
  
  const [url, setUrl] = useState(defaultDocsUrl)
  const [currentUrl, setCurrentUrl] = useState(defaultDocsUrl)

  const handleGo = () => {
    setCurrentUrl(url)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleGo()
    }
  }

  return (
    <div className="flex flex-col h-full w-full bg-[#1e1e2e] text-[#cdd6f4]">
      {/* Toolbar */}
      <div className="flex items-center gap-2 p-2 bg-[#181825] border-b border-[#313244]">
        <span className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider ml-1 mr-2">API Docs</span>
        <input
          type="text"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={handleKeyDown}
          className="flex-1 bg-[#313244] text-[#cdd6f4] text-xs px-2 py-1 rounded outline-none focus:ring-1 focus:ring-[#89b4fa]"
          placeholder="Enter API Docs URL..."
        />
        <button
          onClick={handleGo}
          className="p-1 hover:bg-[#313244] text-[#a6adc8] hover:text-[#cdd6f4] rounded transition-colors"
          title="Reload"
        >
          <RefreshCw size={14} />
        </button>
        <a
          href={currentUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="p-1 hover:bg-[#313244] text-[#a6adc8] hover:text-[#cdd6f4] rounded transition-colors"
          title="Open in new tab"
        >
          <ExternalLink size={14} />
        </a>
      </div>
      
      {/* Iframe */}
      <div className="flex-1 bg-white">
        <iframe
          src={currentUrl}
          className="w-full h-full border-none"
          title="API Documentation"
          sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
        />
      </div>
    </div>
  )
}
