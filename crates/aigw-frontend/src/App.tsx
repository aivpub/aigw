import {
  BrowserRouter,
  Routes,
  Route,
  Navigate,
  useLocation,
} from "react-router-dom";
import { AuthProvider, useAuth } from "@/hooks/use-auth";
import { Shell } from "@/components/layout/shell";
import { LoginPage } from "@/pages/login";
import { UsagePage } from "@/pages/usage";
import { KeysPage } from "@/pages/keys";
import { ModelsPage } from "@/pages/models";
import { UsersPage } from "@/pages/users";
import { OrgsPage } from "@/pages/orgs";
import { TeamsPage } from "@/pages/teams";
import { SpendLogsPage } from "@/pages/spend-logs";
import { PlaygroundPage } from "@/pages/playground";
import { RouterSettingsPage } from "@/pages/router-settings";
import { JobsPage } from "@/pages/jobs";
import { JobDetailPage } from "@/pages/jobs/job-detail";
import { ErrorBoundary } from "@/pages/jobs/job-detail";
import { BudgetsPage } from "@/pages/budgets";

function RequireAuth({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isLoading } = useAuth();
  const location = useLocation();

  // Don't redirect while auth check is in flight — avoids flash redirects
  if (isLoading) return null;

  if (!isAuthenticated) {
    const redirect =
      location.pathname !== "/dash" ? location.pathname : "/dash/usage";
    return (
      <Navigate
        to={`/dash/login?redirect=${encodeURIComponent(redirect)}`}
        replace
      />
    );
  }

  return <>{children}</>;
}

function AuthGate() {
  return <Navigate to="/dash/login" replace />;
}

export default function App() {
  return (
    <BrowserRouter>
      <AuthProvider>
        <Routes>
          {/* Login — no sidebar */}
          <Route path="/dash/login" element={<LoginPage />} />

          {/* Protected routes with Shell layout */}
          <Route
            path="/dash"
            element={
              <RequireAuth>
                <Shell />
              </RequireAuth>
            }
          >
            <Route index element={<Navigate to="usage" replace />} />
            <Route path="usage" element={<UsagePage />} />
            <Route path="keys" element={<KeysPage />} />
            <Route path="models" element={<ModelsPage />} />
            <Route path="users" element={<UsersPage />} />
            <Route path="orgs" element={<OrgsPage />} />
            <Route path="teams" element={<TeamsPage />} />
            <Route path="spend-logs" element={<SpendLogsPage />} />
            <Route path="playground" element={<PlaygroundPage />} />
            <Route path="router-settings" element={<RouterSettingsPage />} />
            <Route path="jobs" element={<JobsPage />} />
            <Route
              path="jobs/:jobId"
              element={
                <ErrorBoundary>
                  <JobDetailPage />
                </ErrorBoundary>
              }
            />
            <Route path="budgets" element={<BudgetsPage />} />
          </Route>

          {/* Catch-all: authenticated pages redirect to /dash; unauthenticated to login */}
          <Route path="*" element={<AuthGate />} />
        </Routes>
      </AuthProvider>
    </BrowserRouter>
  );
}
