import { NavLink } from "react-router-dom";
import { Key, Box, Gamepad2, BarChart3, ScrollText, Users, Users2, Building2, Shuffle } from "lucide-react";
import { cn } from "@/lib/utils";

interface NavGroup {
  title: string;
  items: { to: string; label: string; icon: React.ComponentType<{ className?: string }> }[];
}

const navGroups: NavGroup[] = [
  {
    title: "AI GATEWAY",
    items: [
      { to: "/dash/keys", label: "Virtual Keys", icon: Key },
      { to: "/dash/models", label: "Models", icon: Box },
      { to: "/dash/playground", label: "Playground", icon: Gamepad2 },
    ],
  },
  {
    title: "OBSERVABILITY",
    items: [
      { to: "/dash/usage", label: "Usage", icon: BarChart3 },
      { to: "/dash/spend-logs", label: "Spend Logs", icon: ScrollText },
    ],
  },
  {
    title: "ACCESS CONTROL",
    items: [
      { to: "/dash/router-settings", label: "Router Settings", icon: Shuffle },
      { to: "/dash/users", label: "Users", icon: Users },
      { to: "/dash/teams", label: "Teams", icon: Users2 },
      { to: "/dash/orgs", label: "Organizations", icon: Building2 },
    ],
  },
];

interface SidebarProps {
  open: boolean;
  onClose: () => void;
}

export function Sidebar({ open, onClose }: SidebarProps) {
  return (
    <>
      {/* Mobile overlay */}
      {open && (
        <div
          className="fixed inset-0 z-40 bg-black/50 lg:hidden"
          onClick={onClose}
        />
      )}

      <aside
        className={cn(
          "fixed left-0 top-0 z-50 h-screen w-56 border-r bg-card transition-transform duration-200 flex flex-col",
          "lg:translate-x-0",
          open ? "translate-x-0" : "-translate-x-full",
        )}
      >
        <div className="flex h-14 items-center border-b px-4 shrink-0">
          <span className="text-lg font-bold tracking-tight">aigw</span>
          <span className="ml-2 text-xs text-muted-foreground">Admin</span>
        </div>
        <nav className="flex-1 overflow-y-auto p-3 space-y-4">
          {navGroups.map((group) => (
            <div key={group.title}>
              <h3 className="mb-1 px-3 text-[10px] font-medium uppercase tracking-[0.05em] text-[#6b7280]">
                {group.title}
              </h3>
              <div className="flex flex-col gap-0.5">
                {group.items.map((item) => (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    onClick={onClose}
                    className={({ isActive }) =>
                      cn(
                        "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                        isActive
                          ? "bg-primary text-primary-foreground"
                          : "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                      )
                    }
                  >
                    <item.icon className="h-4 w-4 shrink-0" />
                    {item.label}
                  </NavLink>
                ))}
              </div>
            </div>
          ))}
        </nav>
      </aside>
    </>
  );
}
