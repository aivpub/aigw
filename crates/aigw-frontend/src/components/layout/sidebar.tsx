import { NavLink } from "react-router-dom";
import { useTranslation } from "react-i18next";
import {
  Key,
  Box,
  Gamepad2,
  BarChart3,
  ScrollText,
  Users,
  Users2,
  Building2,
  Settings,
  Activity,
  ChevronLeft,
  ChevronRight,
  PiggyBank,
  Network,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";

interface NavGroup {
  title: string;
  items: {
    to: string;
    labelKey: string; // i18n dot-key (see sidebar.nav.* / sidebar.group.*)
    icon: React.ComponentType<{ className?: string }>;
  }[];
}

function useNavGroups(): NavGroup[] {
  const { t } = useTranslation();
  return [
    {
      title: t("sidebar.groups.aiGateway"),
      items: [
        { to: "/dash/keys", labelKey: "sidebar.nav.keys", icon: Key },
        { to: "/dash/models", labelKey: "sidebar.nav.models", icon: Box },
        {
          to: "/dash/playground",
          labelKey: "sidebar.nav.playground",
          icon: Gamepad2,
        },
      ],
    },
    {
      title: t("sidebar.groups.observability"),
      items: [
        { to: "/dash/usage", labelKey: "sidebar.nav.usage", icon: BarChart3 },
        {
          to: "/dash/spend-logs",
          labelKey: "sidebar.nav.spendLogs",
          icon: ScrollText,
        },
      ],
    },
    {
      title: t("sidebar.groups.accessControl"),
      items: [
        { to: "/dash/users", labelKey: "sidebar.nav.users", icon: Users },
        { to: "/dash/teams", labelKey: "sidebar.nav.teams", icon: Users2 },
        {
          to: "/dash/orgs",
          labelKey: "sidebar.nav.organizations",
          icon: Building2,
        },
      ],
    },
    {
      title: t("sidebar.groups.settings"),
      items: [
        {
          to: "/dash/router-settings",
          labelKey: "sidebar.nav.routerSettings",
          icon: Settings,
        },
        { to: "/dash/proxies", labelKey: "sidebar.nav.proxies", icon: Network },
        { to: "/dash/jobs", labelKey: "sidebar.nav.jobs", icon: Activity },
        {
          to: "/dash/budgets",
          labelKey: "sidebar.nav.budgets",
          icon: PiggyBank,
        },
      ],
    },
  ];
}

interface SidebarProps {
  open: boolean;
  onClose: () => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
}

export function Sidebar({
  open,
  onClose,
  collapsed,
  onToggleCollapse,
}: SidebarProps) {
  const { t } = useTranslation();
  const navGroups = useNavGroups();

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
          "fixed left-0 top-0 z-50 h-screen border-r bg-card transition-all duration-200 flex flex-col",
          collapsed ? "w-14" : "w-56",
          "lg:translate-x-0",
          open ? "translate-x-0" : "-translate-x-full",
        )}
      >
        {/* Brand area */}
        <div
          className={cn(
            "flex h-14 items-center border-b shrink-0 overflow-hidden",
            collapsed ? "justify-center px-2" : "px-4",
          )}
        >
          <span
            className={cn(
              "text-lg font-bold tracking-tight transition-opacity",
              collapsed ? "opacity-0 w-0" : "",
            )}
          >
            {t("sidebar.brand")}
          </span>
          <span
            className={cn(
              "text-xs text-muted-foreground transition-opacity",
              collapsed ? "opacity-0 w-0" : "ml-2",
            )}
          >
            {t("sidebar.admin")}
          </span>
        </div>

        <nav className="flex-1 overflow-y-auto overflow-x-hidden p-2 space-y-4">
          {navGroups.map((group) => (
            <div key={group.title}>
              {!collapsed && (
                <h3 className="mb-1 px-3 text-[10px] font-medium uppercase tracking-[0.05em] text-[#6b7280]">
                  {group.title}
                </h3>
              )}
              <div className="flex flex-col gap-0.5">
                {group.items.map((item) => (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    onClick={onClose}
                    title={collapsed ? t(item.labelKey as never) : undefined}
                    className={({ isActive }) =>
                      cn(
                        "flex items-center gap-3 rounded-md text-sm font-medium transition-colors",
                        collapsed ? "justify-center px-0 py-2" : "px-3 py-2",
                        isActive
                          ? "bg-primary text-primary-foreground"
                          : "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                      )
                    }
                  >
                    <item.icon className="h-4 w-4 shrink-0" />
                    {!collapsed && t(item.labelKey as never)}
                  </NavLink>
                ))}
              </div>
            </div>
          ))}
        </nav>

        {/* Collapse toggle button — desktop only */}
        <div className="hidden lg:flex border-t p-1.5 shrink-0">
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-full flex items-center justify-center hover:bg-accent"
            onClick={onToggleCollapse}
            title={
              collapsed
                ? t("sidebar.expandSidebar")
                : t("sidebar.collapseSidebar")
            }
          >
            {collapsed ? (
              <ChevronRight className="h-4 w-4" />
            ) : (
              <ChevronLeft className="h-4 w-4" />
            )}
          </Button>
        </div>
      </aside>
    </>
  );
}
