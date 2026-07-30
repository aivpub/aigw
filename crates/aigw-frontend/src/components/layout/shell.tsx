import { useState, useCallback, useEffect } from "react";
import { Outlet } from "react-router-dom";
import { Sidebar } from "./sidebar";
import { Header } from "./header";

const COLLAPSED_KEY = "aigw-sidebar-collapsed";

function loadCollapsed(): boolean {
  try {
    const v = localStorage.getItem(COLLAPSED_KEY);
    return v === "true";
  } catch {
    return false;
  }
}

function saveCollapsed(v: boolean) {
  try {
    localStorage.setItem(COLLAPSED_KEY, String(v));
  } catch { /* noop */ }
}

export function Shell() {
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [collapsed, setCollapsed] = useState(loadCollapsed);

  const toggleCollapse = useCallback(() => {
    setCollapsed(prev => {
      const next = !prev;
      saveCollapsed(next);
      return next;
    });
  }, []);

  // Sync with localStorage when another tab changes it (e.g. via storage event)
  useEffect(() => {
    const handler = (e: StorageEvent) => {
      if (e.key === COLLAPSED_KEY) {
        setCollapsed(e.newValue === "true");
      }
    };
    window.addEventListener("storage", handler);
    return () => window.removeEventListener("storage", handler);
  }, []);

  return (
    <div className="min-h-screen bg-background">
      <Sidebar
        open={sidebarOpen}
        onClose={() => setSidebarOpen(false)}
        collapsed={collapsed}
        onToggleCollapse={toggleCollapse}
      />
      <div className={collapsed ? "lg:ml-14" : "lg:ml-56"}>
        <Header
          onMenuClick={() => setSidebarOpen(true)}
          collapsed={collapsed}
          onToggleCollapse={toggleCollapse}
        />
        <main className="p-4 lg:p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
