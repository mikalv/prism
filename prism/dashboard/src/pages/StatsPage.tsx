import { useState, useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { RefreshCw } from 'lucide-react'
import { LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts'

interface HistoryPoint {
  time: string
  hitRate: number
  hits: number
  misses: number
}

const MAX_HISTORY = 20

export function StatsPage() {
  const queryClient = useQueryClient()
  const [history, setHistory] = useState<HistoryPoint[]>([])

  const serverQuery = useQuery({
    queryKey: ['server-info'],
    queryFn: api.getServerInfo,
  })

  const cacheQuery = useQuery({
    queryKey: ['cache-stats'],
    queryFn: api.getCacheStats,
    refetchInterval: 5000,
  })

  // Track cache stats history
  useEffect(() => {
    if (cacheQuery.data) {
      const point: HistoryPoint = {
        time: new Date().toLocaleTimeString(),
        hitRate: (cacheQuery.data.hit_rate ?? 0) * 100,
        hits: cacheQuery.data.hits,
        misses: cacheQuery.data.misses,
      }
      setHistory((prev) => [...prev.slice(-(MAX_HISTORY - 1)), point])
    }
  }, [cacheQuery.data])

  const handleRefresh = () => {
    queryClient.invalidateQueries({ queryKey: ['server-info'] })
    queryClient.invalidateQueries({ queryKey: ['cache-stats'] })
  }

  const isRefreshing = serverQuery.isFetching || cacheQuery.isFetching

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-bold">Dashboard</h2>
        <Button
          variant="outline"
          size="sm"
          onClick={handleRefresh}
          disabled={isRefreshing}
        >
          <RefreshCw className={`mr-2 h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
          Refresh
        </Button>
      </div>

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
        <Card>
          <CardHeader>
            <CardTitle>Server</CardTitle>
            <CardDescription>Prism server information</CardDescription>
          </CardHeader>
          <CardContent>
            {serverQuery.isLoading ? (
              <Skeleton className="h-8 w-32" />
            ) : serverQuery.error ? (
              <p className="text-destructive">Failed to load</p>
            ) : (
              <div className="space-y-1">
                <p className="text-sm text-muted-foreground">Name</p>
                <p className="font-medium">{serverQuery.data?.name}</p>
                <p className="text-sm text-muted-foreground mt-2">Version</p>
                <p className="font-medium">{serverQuery.data?.version}</p>
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Cache</CardTitle>
            <CardDescription>Embedding cache performance</CardDescription>
          </CardHeader>
          <CardContent>
            {cacheQuery.isLoading ? (
              <Skeleton className="h-8 w-32" />
            ) : cacheQuery.error ? (
              <p className="text-destructive">Failed to load</p>
            ) : (
              <div className="space-y-1">
                <p className="text-sm text-muted-foreground">Hit Rate</p>
                <p className="text-2xl font-bold">
                  {((cacheQuery.data?.hit_rate ?? 0) * 100).toFixed(1)}%
                </p>
                <div className="flex gap-4 mt-2 text-sm">
                  <div>
                    <span className="text-muted-foreground">Hits: </span>
                    <span className="font-medium">{cacheQuery.data?.hits?.toLocaleString()}</span>
                  </div>
                  <div>
                    <span className="text-muted-foreground">Misses: </span>
                    <span className="font-medium">{cacheQuery.data?.misses?.toLocaleString()}</span>
                  </div>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {history.length > 1 && (
        <Card>
          <CardHeader>
            <CardTitle>Cache Hit Rate</CardTitle>
            <CardDescription>
              Last {history.length} measurements (updates every 5s)
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="h-64">
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={history}>
                  <XAxis
                    dataKey="time"
                    tick={{ fontSize: 12 }}
                    interval="preserveStartEnd"
                  />
                  <YAxis
                    domain={[0, 100]}
                    tick={{ fontSize: 12 }}
                    tickFormatter={(v) => `${v}%`}
                  />
                  <Tooltip
                    formatter={(value) => [`${Number(value).toFixed(1)}%`, 'Hit Rate']}
                    labelFormatter={(label) => `Time: ${label}`}
                  />
                  <Line
                    type="monotone"
                    dataKey="hitRate"
                    stroke="hsl(var(--primary))"
                    strokeWidth={2}
                    dot={false}
                  />
                </LineChart>
              </ResponsiveContainer>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
