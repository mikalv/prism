import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'

export function IndexPage() {
  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold">Index Inspector</h2>

      <Card>
        <CardHeader>
          <CardTitle>
            Coming Soon
            <Badge variant="secondary" className="ml-2">
              Issue #24
            </Badge>
          </CardTitle>
          <CardDescription>
            Deep index inspection features require backend API implementation
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4 text-muted-foreground">
          <p>Planned features:</p>
          <ul className="list-disc list-inside space-y-1">
            <li>Term enumeration per field (top-k terms)</li>
            <li>Tantivy segment statistics</li>
            <li>Document reconstruction from stored fields</li>
          </ul>
          <p className="text-sm">
            See <code>prism/src/backends/text.rs</code> for the CollectionIndex
            struct that provides access to Tantivy internals.
          </p>
        </CardContent>
      </Card>
    </div>
  )
}
