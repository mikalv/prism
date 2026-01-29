import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { DashboardLayout } from '@/layouts/DashboardLayout'
import { StatsPage } from '@/pages/StatsPage'
import { CollectionsPage } from '@/pages/CollectionsPage'
import { AggregationsPage } from '@/pages/AggregationsPage'
import { SearchPage } from '@/pages/SearchPage'
import { IndexPage } from '@/pages/IndexPage'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
})

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          <Route element={<DashboardLayout />}>
            <Route path="/" element={<StatsPage />} />
            <Route path="/collections" element={<CollectionsPage />} />
            <Route path="/search" element={<SearchPage />} />
            <Route path="/aggregations" element={<AggregationsPage />} />
            <Route path="/index" element={<IndexPage />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  )
}

export default App
