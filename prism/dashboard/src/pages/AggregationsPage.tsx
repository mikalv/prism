import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts'

export function AggregationsPage() {
  const [collection, setCollection] = useState<string | null>(null)
  const [field, setField] = useState('')
  const [runQuery, setRunQuery] = useState(false)

  const collectionsQuery = useQuery({
    queryKey: ['collections'],
    queryFn: api.listCollections,
  })

  const schemaQuery = useQuery({
    queryKey: ['schema', collection],
    queryFn: () => api.getCollectionSchema(collection!),
    enabled: !!collection,
  })

  const aggQuery = useQuery({
    queryKey: ['aggregation', collection, field],
    queryFn: () => api.runAggregation(collection!, field),
    enabled: runQuery && !!collection && !!field,
  })

  const handleRun = () => {
    if (collection && field) {
      setRunQuery(true)
    }
  }

  const textFields = schemaQuery.data?.fields.filter(
    (f) => f.indexed && !f.vector_dimensions
  )

  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold">Aggregations</h2>

      <Card>
        <CardHeader>
          <CardTitle>Terms Aggregation</CardTitle>
          <CardDescription>
            Analyze term distribution across a field
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-wrap gap-4">
            <div className="space-y-2">
              <label className="text-sm font-medium">Collection</label>
              <div className="flex flex-wrap gap-2">
                {collectionsQuery.data?.map((name) => (
                  <Button
                    key={name}
                    variant={collection === name ? 'default' : 'outline'}
                    size="sm"
                    onClick={() => {
                      setCollection(name)
                      setField('')
                      setRunQuery(false)
                    }}
                  >
                    {name}
                  </Button>
                ))}
              </div>
            </div>

            {collection && textFields && (
              <div className="space-y-2">
                <label className="text-sm font-medium">Field</label>
                <div className="flex flex-wrap gap-2">
                  {textFields.map((f) => (
                    <Button
                      key={f.name}
                      variant={field === f.name ? 'default' : 'outline'}
                      size="sm"
                      onClick={() => {
                        setField(f.name)
                        setRunQuery(false)
                      }}
                    >
                      {f.name}
                    </Button>
                  ))}
                </div>
              </div>
            )}
          </div>

          <div className="flex gap-2">
            <Button onClick={handleRun} disabled={!collection || !field}>
              Run Aggregation
            </Button>
          </div>
        </CardContent>
      </Card>

      {runQuery && (
        <Card>
          <CardHeader>
            <CardTitle>Results</CardTitle>
            <CardDescription>
              Top terms for {field} in {collection}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {aggQuery.isLoading ? (
              <Skeleton className="h-64 w-full" />
            ) : aggQuery.error ? (
              <p className="text-destructive">Failed to run aggregation</p>
            ) : aggQuery.data?.buckets.length === 0 ? (
              <p className="text-muted-foreground">No results</p>
            ) : (
              <div className="h-64">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={aggQuery.data?.buckets}>
                    <XAxis dataKey="key" />
                    <YAxis />
                    <Tooltip />
                    <Bar dataKey="doc_count" fill="hsl(var(--primary))" />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  )
}
