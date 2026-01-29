import { NavLink, Outlet } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarInset,
} from '@/components/ui/sidebar'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { BarChart3, Database, Layers, Activity, Moon, Sun, Search, Circle } from 'lucide-react'
import { useTheme } from '@/hooks/use-theme'
import { api } from '@/api/client'

const navItems = [
  { to: '/', icon: Activity, label: 'Stats' },
  { to: '/collections', icon: Database, label: 'Collections' },
  { to: '/search', icon: Search, label: 'Search' },
  { to: '/aggregations', icon: BarChart3, label: 'Aggregations' },
  { to: '/index', icon: Layers, label: 'Index' },
]

export function DashboardLayout() {
  const { theme, setTheme } = useTheme()

  const healthQuery = useQuery({
    queryKey: ['health'],
    queryFn: api.getServerInfo,
    refetchInterval: 10_000,
    retry: false,
  })

  const toggleTheme = () => {
    setTheme(theme === 'dark' ? 'light' : 'dark')
  }

  const isConnected = healthQuery.isSuccess
  const isLoading = healthQuery.isLoading

  return (
    <SidebarProvider>
      <Sidebar>
        <SidebarHeader className="border-b px-4 py-3">
          <div className="flex items-center justify-between">
            <h1 className="text-lg font-semibold">Prism</h1>
            <Badge
              variant={isConnected ? 'default' : 'destructive'}
              className="text-xs"
            >
              <Circle
                className={`mr-1 h-2 w-2 fill-current ${
                  isLoading ? 'animate-pulse' : ''
                }`}
              />
              {isLoading ? 'Connecting' : isConnected ? 'Connected' : 'Offline'}
            </Badge>
          </div>
        </SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupLabel>Navigation</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {navItems.map((item) => (
                  <SidebarMenuItem key={item.to}>
                    <SidebarMenuButton asChild>
                      <NavLink
                        to={item.to}
                        className={({ isActive }) =>
                          isActive ? 'bg-accent' : ''
                        }
                      >
                        <item.icon className="mr-2 h-4 w-4" />
                        {item.label}
                      </NavLink>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>
        <SidebarFooter className="border-t p-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={toggleTheme}
            className="w-full justify-start"
          >
            {theme === 'dark' ? (
              <>
                <Sun className="mr-2 h-4 w-4" />
                Light mode
              </>
            ) : (
              <>
                <Moon className="mr-2 h-4 w-4" />
                Dark mode
              </>
            )}
          </Button>
        </SidebarFooter>
      </Sidebar>
      <SidebarInset>
        <main className="flex-1 p-6">
          <Outlet />
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}
