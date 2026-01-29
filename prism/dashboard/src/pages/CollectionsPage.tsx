import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, type FieldSchema } from '@/api/client'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { PageHeader } from '@/components/PageHeader'

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`
}

function FieldTypeBadge({ field }: { field: FieldSchema }) {
  const variant = field.vector_dimensions ? 'default' : 'secondary'
  const label = field.vector_dimensions
    ? `vector[${field.vector_dimensions}]`
    : field.field_type
  return <Badge variant={variant}>{label}</Badge>
}

export function CollectionsPage() {
  const queryClient = useQueryClient()
  const [selected, setSelected] = useState<string | null>(null)

  const collectionsQuery = useQuery({
    queryKey: ['collections'],
    queryFn: api.listCollections,
  })

  const schemaQuery = useQuery({
    queryKey: ['schema', selected],
    queryFn: () => api.getCollectionSchema(selected!),
    enabled: !!selected,
  })

  const statsQuery = useQuery({
    queryKey: ['stats', selected],
    queryFn: () => api.getCollectionStats(selected!),
    enabled: !!selected,
  })

  const sampleQuery = useQuery({
    queryKey: ['sample', selected],
    queryFn: () => api.sampleDocuments(selected!, 5),
    enabled: !!selected,
  })

  const handleRefresh = () => {
    queryClient.invalidateQueries({ queryKey: ['collections'] })
    if (selected) {
      queryClient.invalidateQueries({ queryKey: ['schema', selected] })
      queryClient.invalidateQueries({ queryKey: ['stats', selected] })
      queryClient.invalidateQueries({ queryKey: ['sample', selected] })
    }
  }

  const isRefreshing = collectionsQuery.isFetching || schemaQuery.isFetching || statsQuery.isFetching

  return (
    <div className="space-y-6">
      <PageHeader
        title="Collections"
        onRefresh={handleRefresh}
        isRefreshing={isRefreshing}
      />

      <div className="grid gap-6 lg:grid-cols-[280px_1fr]">
        <Card>
          <CardHeader>
            <CardTitle>Collections</CardTitle>
            <CardDescription>Select a collection to inspect</CardDescription>
          </CardHeader>
          <CardContent>
            {collectionsQuery.isLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-8 w-full" />
                <Skeleton className="h-8 w-full" />
              </div>
            ) : collectionsQuery.error ? (
              <p className="text-destructive">Failed to load collections</p>
            ) : collectionsQuery.data?.length === 0 ? (
              <p className="text-muted-foreground">No collections found</p>
            ) : (
              <div className="space-y-1">
                {collectionsQuery.data?.map((name) => (
                  <Button
                    key={name}
                    variant={selected === name ? 'secondary' : 'ghost'}
                    className="w-full justify-start"
                    onClick={() => setSelected(name)}
                  >
                    {name}
                  </Button>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {selected && (
          <div className="space-y-6">
            <Card>
              <CardHeader>
                <CardTitle>{selected}</CardTitle>
                <CardDescription>Collection statistics</CardDescription>
              </CardHeader>
              <CardContent>
                {statsQuery.isLoading ? (
                  <Skeleton className="h-12 w-48" />
                ) : statsQuery.error ? (
                  <p className="text-destructive">Failed to load stats</p>
                ) : (
                  <div className="flex gap-8">
                    <div>
                      <p className="text-sm text-muted-foreground">Documents</p>
                      <p className="text-2xl font-bold">
                        {statsQuery.data?.document_count.toLocaleString()}
                      </p>
                    </div>
                    <div>
                      <p className="text-sm text-muted-foreground">Storage</p>
                      <p className="text-2xl font-bold">
                        {formatBytes(statsQuery.data?.storage_size_bytes ?? 0)}
                      </p>
                    </div>
                  </div>
                )}
              </CardContent>
            </Card>

            <Tabs defaultValue="schema">
              <TabsList>
                <TabsTrigger value="schema">Schema</TabsTrigger>
                <TabsTrigger value="documents">Sample Documents</TabsTrigger>
              </TabsList>

              <TabsContent value="schema">
                <Card>
                  <CardHeader>
                    <CardTitle>Schema</CardTitle>
                    <CardDescription>
                      Dynamic field definitions for this collection
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    {schemaQuery.isLoading ? (
                      <Skeleton className="h-32 w-full" />
                    ) : schemaQuery.error ? (
                      <p className="text-destructive">Failed to load schema</p>
                    ) : schemaQuery.data?.fields.length === 0 ? (
                      <p className="text-muted-foreground">No fields defined</p>
                    ) : (
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>Field</TableHead>
                            <TableHead>Type</TableHead>
                            <TableHead>Indexed</TableHead>
                            <TableHead>Stored</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {schemaQuery.data?.fields.map((field) => (
                            <TableRow key={field.name}>
                              <TableCell className="font-mono">{field.name}</TableCell>
                              <TableCell>
                                <FieldTypeBadge field={field} />
                              </TableCell>
                              <TableCell>
                                {field.indexed ? (
                                  <Badge variant="outline">Yes</Badge>
                                ) : (
                                  <span className="text-muted-foreground">No</span>
                                )}
                              </TableCell>
                              <TableCell>
                                {field.stored ? (
                                  <Badge variant="outline">Yes</Badge>
                                ) : (
                                  <span className="text-muted-foreground">No</span>
                                )}
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    )}
                  </CardContent>
                </Card>
              </TabsContent>

              <TabsContent value="documents">
                <Card>
                  <CardHeader>
                    <CardTitle>Sample Documents</CardTitle>
                    <CardDescription>
                      Preview documents from this collection
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    {sampleQuery.isLoading ? (
                      <div className="space-y-2">
                        <Skeleton className="h-20 w-full" />
                        <Skeleton className="h-20 w-full" />
                      </div>
                    ) : sampleQuery.error ? (
                      <p className="text-destructive">Failed to load documents</p>
                    ) : sampleQuery.data?.hits.length === 0 ? (
                      <p className="text-muted-foreground">No documents found</p>
                    ) : (
                      <ScrollArea className="h-[400px]">
                        <div className="space-y-3">
                          {sampleQuery.data?.hits.map((hit, i) => (
                            <div
                              key={hit.id ?? i}
                              className="border rounded-lg p-3 space-y-2"
                            >
                              {hit.id && (
                                <Badge variant="outline" className="mb-2">
                                  ID: {hit.id}
                                </Badge>
                              )}
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
              </TabsContent>
            </Tabs>
          </div>
        )}
      </div>
    </div>
  )
}
