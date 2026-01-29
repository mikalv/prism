import { NavLink, Outlet } from 'react-router-dom'
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
import { BarChart3, Database, Layers, Activity, Moon, Sun, Search } from 'lucide-react'
import { useTheme } from '@/hooks/use-theme'

const navItems = [
  { to: '/', icon: Activity, label: 'Stats' },
  { to: '/collections', icon: Database, label: 'Collections' },
  { to: '/search', icon: Search, label: 'Search' },
  { to: '/aggregations', icon: BarChart3, label: 'Aggregations' },
  { to: '/index', icon: Layers, label: 'Index' },
]

export function DashboardLayout() {
  const { theme, setTheme } = useTheme()

  const toggleTheme = () => {
    setTheme(theme === 'dark' ? 'light' : 'dark')
  }

  return (
    <SidebarProvider>
      <Sidebar>
        <SidebarHeader className="border-b px-4 py-3">
          <h1 className="text-lg font-semibold">Prism Dashboard</h1>
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
