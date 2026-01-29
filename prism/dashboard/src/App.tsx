import { lazy, Suspense } from 'react'
import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { DashboardLayout } from '@/layouts/DashboardLayout'
import { Skeleton } from '@/components/ui/skeleton'

const StatsPage = lazy(() => import('@/pages/StatsPage').then(m => ({ default: m.StatsPage })))
const CollectionsPage = lazy(() => import('@/pages/CollectionsPage').then(m => ({ default: m.CollectionsPage })))
const SearchPage = lazy(() => import('@/pages/SearchPage').then(m => ({ default: m.SearchPage })))
const AggregationsPage = lazy(() => import('@/pages/AggregationsPage').then(m => ({ default: m.AggregationsPage })))
const IndexPage = lazy(() => import('@/pages/IndexPage').then(m => ({ default: m.IndexPage })))

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
})

function PageLoader() {
  return (
    <div className="space-y-4">
      <Skeleton className="h-8 w-48" />
      <Skeleton className="h-64 w-full" />
    </div>
  )
}

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          <Route element={<DashboardLayout />}>
            <Route
              path="/"
              element={
                <Suspense fallback={<PageLoader />}>
                  <StatsPage />
                </Suspense>
              }
            />
            <Route
              path="/collections"
              element={
                <Suspense fallback={<PageLoader />}>
                  <CollectionsPage />
                </Suspense>
              }
            />
            <Route
              path="/search"
              element={
                <Suspense fallback={<PageLoader />}>
                  <SearchPage />
                </Suspense>
              }
            />
            <Route
              path="/aggregations"
              element={
                <Suspense fallback={<PageLoader />}>
                  <AggregationsPage />
                </Suspense>
              }
            />
            <Route
              path="/index"
              element={
                <Suspense fallback={<PageLoader />}>
                  <IndexPage />
                </Suspense>
              }
            />
          </Route>
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  )
}

export default App
