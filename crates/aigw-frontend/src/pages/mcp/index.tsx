import { useTranslation } from "react-i18next";
import { KeyRound, ScrollText, Server, ShieldAlert } from "lucide-react";
import { Navigate, Outlet, useLocation, useNavigate } from "react-router-dom";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAuth } from "@/hooks/use-auth";

const ADMIN_TABS = ["servers", "mine", "calls", "rejections"] as const;
const USER_TABS = ["mine", "calls"] as const;

export function McpPage() {
  const { t } = useTranslation();
  const { userRole } = useAuth();
  const isAdmin = userRole === "proxy_admin";
  const navigate = useNavigate();
  const { pathname } = useLocation();

  const tab = pathname.split("/").filter(Boolean).pop() ?? "";
  const tabs: readonly string[] = isAdmin ? ADMIN_TABS : USER_TABS;

  if (!tabs.includes(tab)) {
    return <Navigate to={`/dash/mcp/${isAdmin ? "servers" : "mine"}`} replace />;
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{t("mcp.title")}</h1>
        <p className="text-sm text-muted-foreground">{t("mcp.subtitle")}</p>
      </div>

      <Tabs value={tab} onValueChange={(v) => navigate(`/dash/mcp/${v}`)} defaultValue={tab}>
        <TabsList className="h-auto min-h-9 max-w-full flex-wrap" data-testid="mcp-tabs">
          {isAdmin && (
            <TabsTrigger value="servers" className="gap-1.5" data-testid="mcp-tab-servers">
              <Server className="h-3.5 w-3.5" />
              {t("mcp.tabs.servers")}
            </TabsTrigger>
          )}
          <TabsTrigger value="mine" className="gap-1.5" data-testid="mcp-tab-mine">
            <KeyRound className="h-3.5 w-3.5" />
            {t("mcp.tabs.mine")}
          </TabsTrigger>
          <TabsTrigger value="calls" className="gap-1.5" data-testid="mcp-tab-calls">
            <ScrollText className="h-3.5 w-3.5" />
            {t("mcp.tabs.calls")}
          </TabsTrigger>
          {isAdmin && (
            <TabsTrigger value="rejections" className="gap-1.5" data-testid="mcp-tab-rejections">
              <ShieldAlert className="h-3.5 w-3.5" />
              {t("mcp.tabs.rejections")}
            </TabsTrigger>
          )}
        </TabsList>
      </Tabs>

      <Outlet />
    </div>
  );
}
