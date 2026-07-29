import { useState, useEffect } from 'react'

export function AdminDashboard() {
  const [health, setHealth] = useState<any>(null)
  const [pipelines, setPipelines] = useState<any>(null)
  const [tasks, setTasks] = useState<any>(null)
  const [lintResult, setLintResult] = useState<any>(null)
  const [loadingLint, setLoadingLint] = useState(false)
  const [error, setError] = useState('')

  const API_BASE_URL = import.meta.env.VITE_API_URL || ''

  useEffect(() => {
    const fetchAdminData = async () => {
      try {
        const [healthRes, pipesRes, tasksRes] = await Promise.all([
          fetch(`${API_BASE_URL}/health`),
          fetch(`${API_BASE_URL}/admin/pipelines`),
          fetch(`${API_BASE_URL}/admin/tasks`)
        ])
        if (healthRes.ok) setHealth(await healthRes.json())
        if (pipesRes.ok) setPipelines(await pipesRes.json())
        if (tasksRes.ok) {
          setTasks(await tasksRes.json())
        }
      } catch (err: any) {
        setError(err.message)
      }
    }
    
    fetchAdminData()
    const interval = setInterval(fetchAdminData, 10000)
    return () => clearInterval(interval)
  }, [])

  const runLint = async () => {
    setLoadingLint(true)
    try {
      const res = await fetch(`${API_BASE_URL}/admin/lint-schemas`)
      setLintResult(await res.json())
    } catch (err: any) {
      setLintResult({ error: err.message })
    } finally {
      setLoadingLint(false)
    }
  }

  return (
    <div className="flex flex-col h-full w-full bg-[#1e1e2e] text-[#cdd6f4] p-4 font-mono text-sm overflow-auto">
      <h2 className="text-[#a6adc8] text-xs font-semibold uppercase tracking-wider mb-4 border-b border-[#313244] pb-2">Admin Dashboard</h2>
      
      {error && <div className="text-red-400 mb-4">{error}</div>}
      
      <div className="flex flex-col gap-4">
        <div className="bg-[#181825] p-4 rounded border border-[#313244] flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <div>
              <div className="font-semibold text-white">Cluster Health</div>
              <div className="text-xs text-[#a6adc8]">Current health status of the cluster.</div>
            </div>
          </div>
          <pre className="text-xs overflow-auto text-green-400 max-h-32 bg-[#11111b] p-2 rounded">
            {health ? JSON.stringify(health, null, 2) : 'Loading...'}
          </pre>
        </div>
        
        <div className="bg-[#181825] p-4 rounded border border-[#313244] flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <div>
              <div className="font-semibold text-white">Pipelines</div>
              <div className="text-xs text-[#a6adc8]">Active data pipelines.</div>
            </div>
          </div>
          <pre className="text-xs overflow-auto text-blue-400 max-h-32 bg-[#11111b] p-2 rounded">
            {pipelines ? JSON.stringify(pipelines, null, 2) : 'Loading...'}
          </pre>
        </div>

        <div className="bg-[#181825] p-4 rounded border border-[#313244] flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <div>
              <div className="font-semibold text-white">Active Tasks</div>
              <div className="text-xs text-[#a6adc8]">Background jobs and system tasks.</div>
            </div>
          </div>
          <pre className="text-xs overflow-auto text-purple-400 max-h-32 bg-[#11111b] p-2 rounded">
            {tasks ? JSON.stringify(tasks, null, 2) : 'Loading...'}
          </pre>
        </div>

        <div className="bg-[#181825] p-4 rounded border border-[#313244] flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <div>
              <div className="font-semibold text-white">Lint Schemas</div>
              <div className="text-xs text-[#a6adc8]">Run schema validation and linting.</div>
            </div>
            <button 
              onClick={runLint}
              disabled={loadingLint}
              className="px-3 py-1 bg-yellow-900/40 text-yellow-400 hover:bg-yellow-900/60 rounded text-xs font-medium transition-colors disabled:opacity-50"
            >
              {loadingLint ? 'Running...' : 'Run Lint'}
            </button>
          </div>
          {lintResult && (
            <pre className="text-xs overflow-auto text-yellow-400 max-h-32 bg-[#11111b] p-2 rounded mt-2">
              {JSON.stringify(lintResult, null, 2)}
            </pre>
          )}
        </div>
      </div>
    </div>
  )
}
