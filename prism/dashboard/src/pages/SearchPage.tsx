import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Search } from 'lucide-react'

export function SearchPage() {
  const [collection, setCollection] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [submitted, setSubmitted] = useState<{ collection: string; query: string } | null>(null)

  const collectionsQuery = useQuery({
    queryKey: ['collections'],
    queryFn: api.listCollections,
  })

  const searchQuery = useQuery({
    queryKey: ['search', submitted?.collection, submitted?.query],
    queryFn: () => api.search(submitted!.collection, submitted!.query),
    enabled: !!submitted,
  })

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault()
    if (collection && query.trim()) {
      setSubmitted({ collection, query: query.trim() })
    }
  }

  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold">Search</h2>

      <Card>
        <CardHeader>
          <CardTitle>Query</CardTitle>
          <CardDescription>
            Search documents in a collection
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSearch} className="space-y-4">
            <div className="space-y-2">
              <label className="text-sm font-medium">Collection</label>
              <div className="flex flex-wrap gap-2">
                {collectionsQuery.isLoading ? (
                  <Skeleton className="h-8 w-24" />
                ) : (
                  collectionsQuery.data?.map((name) => (
                    <Button
                      key={name}
                      type="button"
                      variant={collection === name ? 'default' : 'outline'}
                      size="sm"
                      onClick={() => setCollection(name)}
                    >
                      {name}
                    </Button>
                  ))
                )}
              </div>
            </div>

            <div className="flex gap-2">
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Enter search query..."
                className="flex-1"
              />
              <Button type="submit" disabled={!collection || !query.trim()}>
                <Search className="mr-2 h-4 w-4" />
                Search
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>

      {submitted && (
        <Card>
          <CardHeader>
            <CardTitle>Results</CardTitle>
            <CardDescription>
              Showing results for "{submitted.query}" in {submitted.collection}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {searchQuery.isLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-16 w-full" />
                <Skeleton className="h-16 w-full" />
                <Skeleton className="h-16 w-full" />
              </div>
            ) : searchQuery.error ? (
              <p className="text-destructive">
                Search failed: {(searchQuery.error as Error).message}
              </p>
            ) : searchQuery.data?.hits.length === 0 ? (
              <p className="text-muted-foreground">No results found</p>
            ) : (
              <ScrollArea className="h-[400px]">
                <div className="space-y-3">
                  {searchQuery.data?.hits.map((hit, i) => (
                    <div
                      key={i}
                      className="border rounded-lg p-3 space-y-2"
                    >
                      <div className="flex items-center gap-2">
                        <Badge variant="secondary">
                          Score: {hit.score.toFixed(3)}
                        </Badge>
                        {hit.id && (
                          <Badge variant="outline">ID: {hit.id}</Badge>
                        )}
                      </div>
                      <pre className="text-sm bg-muted p-2 rounded overflow-x-auto">
                        {JSON.stringify(hit.document, null, 2)}
                      </pre>
                    </div>
                  ))}
                </div>
              </ScrollArea>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  )
}
