import { useState, useEffect } from 'react'
import { getCollections } from '@/lib/api'
import { Database, Search, Filter, RefreshCw, X, ChevronRight, MessageSquare, Bot, User, Wrench, Layers } from 'lucide-react'

interface DocumentItem {
  id: string
  score?: number
  snippet?: string
  fields: Record<string, unknown>
}

interface CollectionStats {
  document_count: number
  storage_bytes: number
  segment_count?: number
}

const API_BASE_URL = import.meta.env.VITE_API_URL || ''

export function CollectionBrowser() {
  const [collections, setCollections] = useState<string[]>([])
  const [selectedCollection, setSelectedCollection] = useState<string>('agent_conversations')
  const [documents, setDocuments] = useState<DocumentItem[]>([])
  const [totalDocs, setTotalDocs] = useState<number>(0)
  const [stats, setStats] = useState<CollectionStats | null>(null)
  
  // Search & Filter state
  const [query, setQuery] = useState('')
  const [filterSource, setFilterSource] = useState<string>('all')
  const [filterRole, setFilterRole] = useState<string>('all')
  const [loading, setLoading] = useState(false)
  const [selectedDoc, setSelectedDoc] = useState<DocumentItem | null>(null)

  // Available sources & roles for facets
  const sources = ['all', 'claude_code', 'gemini', 'chatgpt', 'antigravity', 'copilot']
  const roles = ['all', 'user', 'assistant', 'tool']

  useEffect(() => {
    getCollections().then((cols) => {
      setCollections(cols)
      if (cols.length > 0 && !cols.includes(selectedCollection)) {
        setSelectedCollection(cols[0])
      }
    })
  }, [])

  useEffect(() => {
    if (selectedCollection) {
      loadCollectionData(selectedCollection, query, filterSource, filterRole)
    }
  }, [selectedCollection, filterSource, filterRole])

  const loadCollectionData = async (col: string, searchQuery: string, sourceFilter: string, roleFilter: string) => {
    setLoading(true)
    try {
      // Fetch stats
      const statsResp = await fetch(`${API_BASE_URL}/collections/${col}/stats`)
      if (statsResp.ok) {
        const statsData = await statsResp.json()
        setStats(statsData)
      }

      // Fetch documents (via search endpoint or empty match)
      const body: Record<string, unknown> = {
        limit: 50,
        query: searchQuery || '*',
      }

      const mustFilters: Array<{ field: string; value: string }> = []
      if (sourceFilter !== 'all') {
        mustFilters.push({ field: 'source', value: sourceFilter })
      }
      if (roleFilter !== 'all') {
        mustFilters.push({ field: 'role', value: roleFilter })
      }
      if (mustFilters.length > 0) {
        body.filter = { must: mustFilters }
      }

      const searchResp = await fetch(`${API_BASE_URL}/collections/${col}/search`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })

      if (searchResp.ok) {
        const searchData = await searchResp.json()
        setDocuments(searchData.results || [])
        setTotalDocs(searchData.total ?? searchData.results?.length ?? 0)
      } else {
        setDocuments([])
        setTotalDocs(0)
      }
    } catch (err) {
      console.error('Failed to load collection data:', err)
    } finally {
      setLoading(false)
    }
  }

  const handleSearchSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    loadCollectionData(selectedCollection, query, filterSource, filterRole)
  }

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
  }

  const getSourceBadgeClass = (source?: string) => {
    switch (source) {
      case 'claude_code': return 'bg-orange-500/10 text-orange-400 border-orange-500/20'
      case 'chatgpt': return 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20'
      case 'antigravity': return 'bg-blue-500/10 text-blue-400 border-blue-500/20'
      case 'gemini': return 'bg-purple-500/10 text-purple-400 border-purple-500/20'
      case 'copilot': return 'bg-cyan-500/10 text-cyan-400 border-cyan-500/20'
      default: return 'bg-gray-500/10 text-gray-400 border-gray-500/20'
    }
  }

  return (
    <div className="h-full flex flex-col bg-[var(--bg-primary)] text-[var(--text-primary)] font-sans text-sm">
      {/* Top Navbar */}
      <div className="px-4 py-3 border-b border-[var(--border)] bg-[var(--bg-secondary)] flex items-center justify-between gap-4">
        <div className="flex items-center gap-2">
          <Database className="w-5 h-5 text-[var(--accent)]" />
          <h2 className="font-semibold text-base">Collection Browser</h2>
        </div>

        {/* Collection Selector */}
        <div className="flex items-center gap-2">
          <label className="text-xs text-[var(--text-muted)]">Collection:</label>
          <select
            value={selectedCollection}
            onChange={(e) => setSelectedCollection(e.target.value)}
            className="px-3 py-1.5 rounded bg-[var(--bg-tertiary)] border border-[var(--border)] text-xs font-medium focus:outline-none focus:border-[var(--accent)]"
          >
            {collections.map((col) => (
              <option key={col} value={col}>
                {col}
              </option>
            ))}
          </select>
          <button
            onClick={() => loadCollectionData(selectedCollection, query, filterSource, filterRole)}
            className="p-1.5 rounded hover:bg-[var(--bg-tertiary)] text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors"
            title="Refresh"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
          </button>
        </div>
      </div>

      {/* Main Content Area */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Filters & Stats Sidebar */}
        <aside className="w-64 border-r border-[var(--border)] bg-[var(--bg-secondary)]/50 p-4 flex flex-col gap-5 shrink-0 overflow-y-auto">
          {/* Collection Stats Card */}
          {stats && (
            <div className="p-3 rounded-lg bg-[var(--bg-tertiary)] border border-[var(--border)] space-y-2">
              <div className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider flex items-center gap-1.5">
                <Layers className="w-3.5 h-3.5" /> Stats
              </div>
              <div className="grid grid-cols-2 gap-2 text-xs">
                <div>
                  <div className="text-[var(--text-muted)] text-[10px]">Documents</div>
                  <div className="font-semibold text-sm">{stats.document_count.toLocaleString()}</div>
                </div>
                <div>
                  <div className="text-[var(--text-muted)] text-[10px]">Storage</div>
                  <div className="font-semibold text-sm">{formatBytes(stats.storage_bytes)}</div>
                </div>
              </div>
            </div>
          )}

          {/* Search Filter Form */}
          <form onSubmit={handleSearchSubmit} className="space-y-3">
            <div className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider flex items-center gap-1.5">
              <Search className="w-3.5 h-3.5" /> Filter Query
            </div>
            <div className="relative">
              <input
                type="text"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search within collection..."
                className="w-full px-3 py-1.5 text-xs rounded bg-[var(--bg-tertiary)] border border-[var(--border)] pr-8 focus:outline-none focus:border-[var(--accent)]"
              />
              {query && (
                <button
                  type="button"
                  onClick={() => { setQuery(''); loadCollectionData(selectedCollection, '', filterSource, filterRole); }}
                  className="absolute right-2 top-2 text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              )}
            </div>
          </form>

          {/* Facets / Filters */}
          <div className="space-y-4">
            <div className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider flex items-center gap-1.5">
              <Filter className="w-3.5 h-3.5" /> Source
            </div>
            <div className="space-y-1">
              {sources.map((src) => (
                <button
                  key={src}
                  onClick={() => setFilterSource(src)}
                  className={`w-full text-left px-2.5 py-1 rounded text-xs capitalize flex items-center justify-between transition-colors ${
                    filterSource === src
                      ? 'bg-[var(--accent)]/15 text-[var(--accent)] font-medium'
                      : 'hover:bg-[var(--bg-tertiary)] text-[var(--text-secondary)]'
                  }`}
                >
                  <span>{src.replace('_', ' ')}</span>
                  {filterSource === src && <div className="w-1.5 h-1.5 rounded-full bg-[var(--accent)]" />}
                </button>
              ))}
            </div>

            {selectedCollection === 'agent_messages' && (
              <>
                <div className="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wider flex items-center gap-1.5 pt-2">
                  <User className="w-3.5 h-3.5" /> Role
                </div>
                <div className="space-y-1">
                  {roles.map((r) => (
                    <button
                      key={r}
                      onClick={() => setFilterRole(r)}
                      className={`w-full text-left px-2.5 py-1 rounded text-xs capitalize flex items-center justify-between transition-colors ${
                        filterRole === r
                          ? 'bg-[var(--accent)]/15 text-[var(--accent)] font-medium'
                          : 'hover:bg-[var(--bg-tertiary)] text-[var(--text-secondary)]'
                      }`}
                    >
                      <span>{r}</span>
                      {filterRole === r && <div className="w-1.5 h-1.5 rounded-full bg-[var(--accent)]" />}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
        </aside>

        {/* Document List */}
        <div className="flex-1 flex flex-col min-w-0 bg-[var(--bg-primary)]">
          <div className="px-4 py-2 bg-[var(--bg-tertiary)]/30 border-b border-[var(--border)] text-xs text-[var(--text-muted)] flex items-center justify-between">
            <span>Showing {documents.length} of {totalDocs.toLocaleString()} documents</span>
            {loading && <span className="text-[var(--accent)] font-medium">Loading documents...</span>}
          </div>

          <div className="flex-1 overflow-y-auto divide-y divide-[var(--border)]">
            {documents.length === 0 && !loading ? (
              <div className="p-8 text-center text-[var(--text-muted)]">
                No documents found in collection <code className="bg-[var(--bg-tertiary)] px-1.5 py-0.5 rounded">{selectedCollection}</code>
              </div>
            ) : (
              documents.map((doc) => {
                const source = (doc.fields.source as string) || ''
                const role = (doc.fields.role as string) || ''
                const title = (doc.fields.title as string) || (doc.fields.text as string) || doc.id
                const ts = (doc.fields.started_at as string) || (doc.fields.ts as string) || ''
                const model = (doc.fields.model as string) || ''
                const msgCount = doc.fields.msg_count as number

                return (
                  <div
                    key={doc.id}
                    onClick={() => setSelectedDoc(doc)}
                    className={`p-3 hover:bg-[var(--bg-tertiary)]/50 cursor-pointer transition-colors flex items-start justify-between gap-4 ${
                      selectedDoc?.id === doc.id ? 'bg-[var(--accent)]/10' : ''
                    }`}
                  >
                    <div className="space-y-1 min-w-0 flex-1">
                      <div className="flex items-center gap-2 flex-wrap">
                        {source && (
                          <span className={`px-2 py-0.5 rounded text-[10px] font-medium border ${getSourceBadgeClass(source)}`}>
                            {source}
                          </span>
                        )}
                        {role && (
                          <span className="px-1.5 py-0.5 rounded text-[10px] bg-[var(--bg-tertiary)] text-[var(--text-muted)] capitalize flex items-center gap-1">
                            {role === 'user' && <User className="w-2.5 h-2.5" />}
                            {role === 'assistant' && <Bot className="w-2.5 h-2.5" />}
                            {role === 'tool' && <Wrench className="w-2.5 h-2.5" />}
                            {role}
                          </span>
                        )}
                        {model && (
                          <span className="text-[10px] text-[var(--text-muted)] font-mono">
                            {model}
                          </span>
                        )}
                        {msgCount !== undefined && (
                          <span className="text-[10px] text-[var(--text-muted)] flex items-center gap-1">
                            <MessageSquare className="w-2.5 h-2.5" /> {msgCount} msgs
                          </span>
                        )}
                      </div>

                      <div className="font-medium text-xs text-[var(--text-primary)] line-clamp-2 leading-relaxed">
                        {title}
                      </div>

                      {doc.snippet && (
                        <div className="text-[11px] text-[var(--text-secondary)] line-clamp-2 font-mono bg-[var(--bg-tertiary)]/40 p-1.5 rounded">
                          {doc.snippet}
                        </div>
                      )}
                    </div>

                    <div className="text-right shrink-0 flex flex-col items-end gap-1">
                      {ts && (
                        <span className="text-[10px] text-[var(--text-muted)]">
                          {new Date(ts).toLocaleDateString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}
                        </span>
                      )}
                      <ChevronRight className="w-4 h-4 text-[var(--text-muted)] opacity-50" />
                    </div>
                  </div>
                )
              })
            )}
          </div>
        </div>

        {/* Right Document Detail Panel */}
        {selectedDoc && (
          <aside className="w-96 border-l border-[var(--border)] bg-[var(--bg-secondary)] flex flex-col shrink-0 overflow-hidden">
            <div className="px-4 py-3 border-b border-[var(--border)] flex items-center justify-between bg-[var(--bg-tertiary)]/50">
              <span className="font-semibold text-xs text-[var(--text-primary)] truncate">Document Inspector</span>
              <button
                onClick={() => setSelectedDoc(null)}
                className="p-1 rounded hover:bg-[var(--bg-tertiary)] text-[var(--text-muted)]"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="flex-1 p-4 overflow-y-auto space-y-4">
              <div>
                <label className="text-[10px] uppercase font-semibold text-[var(--text-muted)]">Document ID</label>
                <div className="font-mono text-xs text-[var(--accent)] break-all mt-0.5">{selectedDoc.id}</div>
              </div>

              {selectedDoc.fields.text !== undefined && (
                <div>
                  <label className="text-[10px] uppercase font-semibold text-[var(--text-muted)]">Text Content</label>
                  <pre className="mt-1 p-2.5 rounded bg-[var(--bg-tertiary)] text-xs font-mono whitespace-pre-wrap break-words border border-[var(--border)] max-h-72 overflow-y-auto">
                    {String(selectedDoc.fields.text)}
                  </pre>
                </div>
              )}

              <div>
                <label className="text-[10px] uppercase font-semibold text-[var(--text-muted)]">Raw Fields</label>
                <pre className="mt-1 p-2.5 rounded bg-[var(--bg-tertiary)] text-[11px] font-mono text-[var(--text-secondary)] whitespace-pre-wrap break-words border border-[var(--border)]">
                  {JSON.stringify(selectedDoc.fields, null, 2)}
                </pre>
              </div>
            </div>
          </aside>
        )}
      </div>
    </div>
  )
}
