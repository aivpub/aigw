import { Outlet } from "react-router-dom";
import { Sidebar } from "./sidebar";
import { Header } from "./header";

export function Shell() {
  return (
    <div className="min-h-screen bg-background">
      <Sidebar />
      <div className="ml-56">
        <Header />
        <main className="p-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
