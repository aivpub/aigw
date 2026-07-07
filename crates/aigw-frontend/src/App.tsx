import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { Shell } from "@/components/layout/shell";
import { DashboardPage } from "@/pages/dashboard";
import { KeysPage } from "@/pages/keys";
import { ModelsPage } from "@/pages/models";
import { HealthPage } from "@/pages/health";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/admin" element={<Shell />}>
          <Route index element={<Navigate to="dashboard" replace />} />
          <Route path="dashboard" element={<DashboardPage />} />
          <Route path="keys" element={<KeysPage />} />
          <Route path="models" element={<ModelsPage />} />
          <Route path="health" element={<HealthPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/admin" replace />} />
      </Routes>
    </BrowserRouter>
  );
}
