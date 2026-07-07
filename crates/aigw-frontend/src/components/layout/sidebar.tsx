import { NavLink } from "react-router-dom";
import { LayoutDashboard, Key, Box, Activity } from "lucide-react";
import { cn } from "@/lib/utils";

const navItems = [
  { to: "/admin/dashboard", label: "Dashboard", icon: LayoutDashboard },
  { to: "/admin/keys", label: "API Keys", icon: Key },
  { to: "/admin/models", label: "Models", icon: Box },
  { to: "/admin/health", label: "Health", icon: Activity },
];

export function Sidebar() {
  return (
    <aside className="fixed left-0 top-0 z-40 h-screen w-56 border-r bg-card">
      <div className="flex h-14 items-center border-b px-4">
        <span className="text-lg font-bold tracking-tight">aigw</span>
        <span className="ml-2 text-xs text-muted-foreground">Admin</span>
      </div>
      <nav className="flex flex-col gap-1 p-3">
        {navItems.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                isActive
                  ? "bg-primary text-primary-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
              )
            }
          >
            <item.icon className="h-4 w-4" />
            {item.label}
          </NavLink>
        ))}
      </nav>
    </aside>
  );
}
