import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'

export function StatsPage() {
  const serverQuery = useQuery({
    queryKey: ['server-info'],
    queryFn: api.getServerInfo,
  })

  const cacheQuery = useQuery({
    queryKey: ['cache-stats'],
    queryFn: api.getCacheStats,
    refetchInterval: 5000,
  })

  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold">Dashboard</h2>

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
                    <span className="font-medium">{cacheQuery.data?.hits}</span>
                  </div>
                  <div>
                    <span className="text-muted-foreground">Misses: </span>
                    <span className="font-medium">{cacheQuery.data?.misses}</span>
                  </div>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
