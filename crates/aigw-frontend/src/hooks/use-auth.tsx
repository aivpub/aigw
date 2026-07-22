import { createContext, useContext, useState, useCallback, useEffect, type ReactNode } from "react";

interface AuthState {
  isAuthenticated: boolean;
  isLoading: boolean;
  login: (username: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  setUnauthenticated: () => void;
}

const AuthContext = createContext<AuthState>({
  isAuthenticated: false,
  isLoading: true,
  login: async () => {},
  logout: async () => {},
  setUnauthenticated: () => {},
});

export function useAuth() {
  return useContext(AuthContext);
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(false);
  const [isLoading, setIsLoading] = useState(true);

  // Check auth state on mount via cookie-based /v2/login/check
  useEffect(() => {
    let cancelled = false;
    fetch("/v2/login/check", { credentials: "include" })
      .then((res) => {
        if (!cancelled) setIsAuthenticated(res.ok);
      })
      .catch(() => {
        if (!cancelled) setIsAuthenticated(false);
      })
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });
    return () => { cancelled = true; };
  }, []);

  // Listen for global auth:unauthenticated events (fired by handleResponse on 401)
  useEffect(() => {
    const handler = () => setIsAuthenticated(false);
    window.addEventListener("auth:unauthenticated", handler);
    return () => window.removeEventListener("auth:unauthenticated", handler);
  }, []);

  const setUnauthenticated = useCallback(() => {
    setIsAuthenticated(false);
  }, []);

  const login = useCallback(async (username: string, password: string) => {
    const res = await fetch("/v2/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "include",
      body: JSON.stringify({ username, password }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error?.message || "Login failed");
    }
    setIsAuthenticated(true);
  }, []);

  const logout = useCallback(async () => {
    await fetch("/v2/logout", {
      method: "POST",
      credentials: "include",
    });
    setIsAuthenticated(false);
  }, []);

  return (
    <AuthContext.Provider value={{ isAuthenticated, isLoading, login, logout, setUnauthenticated }}>
      {children}
    </AuthContext.Provider>
  );
}
