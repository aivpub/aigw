import { useTranslation } from "react-i18next";
import { Gamepad2 } from "lucide-react";
import { Navigate, Outlet, useLocation, useNavigate } from "react-router-dom";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAuth } from "@/hooks/use-auth";
import { McpPanel } from "./McpPanel";

export function PlaygroundPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const tab = pathname.endsWith("/mcp") ? "mcp" : "chat";

  return (
    <div className="h-[calc(100vh-5rem)] flex flex-col">
      <div className="flex flex-wrap items-center justify-between gap-2 mb-4 shrink-0">
        <div>
          <h1 className="text-xl font-bold tracking-tight flex items-center gap-2">
            <Gamepad2 className="h-5 w-5" />
            {t("playground.title")}
          </h1>
          <p className="text-xs text-muted-foreground">
            {t("playground.description")}
          </p>
        </div>
      </div>

      <Tabs
        value={tab}
        onValueChange={(v) => navigate(`/dash/playground/${v}`)}
        defaultValue={tab}
        className="flex-1 flex flex-col min-h-0"
      >
        <TabsList className="mb-3 shrink-0 self-start">
          <TabsTrigger value="chat" data-testid="playground-tab-chat">
            {t("playground.tabChat")}
          </TabsTrigger>
          <TabsTrigger value="mcp" data-testid="playground-tab-mcp">
            {t("playground.tabMcp")}
          </TabsTrigger>
        </TabsList>

        <Outlet />
      </Tabs>
    </div>
  );
}

export function PlaygroundIndexRedirect() {
  return <Navigate to="/dash/playground/chat" replace />;
}

export function PlaygroundMcpPanel() {
  const { userRole } = useAuth();
  return <McpPanel isAdmin={userRole === "proxy_admin"} />;
}
