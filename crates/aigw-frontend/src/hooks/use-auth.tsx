import {
  createContext,
  useContext,
  useState,
  useCallback,
  useEffect,
  type ReactNode,
} from "react";
import i18n from "@/i18n";

interface AuthState {
  isAuthenticated: boolean;
  isLoading: boolean;
  userRole: string | null;
  userId: string | null;
  login: (username: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  setUnauthenticated: () => void;
}

const AuthContext = createContext<AuthState>({
  isAuthenticated: false,
  isLoading: true,
  userRole: null,
  userId: null,
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
  const [userRole, setUserRole] = useState<string | null>(null);
  const [userId, setUserId] = useState<string | null>(null);

  // Check auth state on mount via cookie-based /v2/login/check
  useEffect(() => {
    let cancelled = false;
    fetch("/v2/login/check", { credentials: "include" })
      .then(async (res) => {
        if (!cancelled) {
          if (res.ok) {
            const data = await res.json().catch(() => ({}));
            setUserRole(data.user_role ?? null);
            setUserId(data.user_id ?? null);
            setIsAuthenticated(true);
          } else {
            setUserRole(null);
            setUserId(null);
            setIsAuthenticated(false);
          }
        }
      })
      .catch(() => {
        if (!cancelled) {
          setUserRole(null);
          setUserId(null);
          setIsAuthenticated(false);
        }
      })
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Listen for global auth:unauthenticated events (fired by handleResponse on 401)
  useEffect(() => {
    const handler = () => {
      setUserRole(null);
      setUserId(null);
      setIsAuthenticated(false);
    };
    window.addEventListener("auth:unauthenticated", handler);
    return () => window.removeEventListener("auth:unauthenticated", handler);
  }, []);

  const setUnauthenticated = useCallback(() => {
    setUserRole(null);
    setUserId(null);
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
      throw new Error(err.error?.message || i18n.t("auth.loginFailed"));
    }
    const data = await res.json().catch(() => ({}));
    setUserRole(data.user_role ?? null);
    setUserId(data.user_id ?? null);
    setIsAuthenticated(true);
  }, []);

  const logout = useCallback(async () => {
    await fetch("/v2/logout", {
      method: "POST",
      credentials: "include",
    });
    setUserRole(null);
    setUserId(null);
    setIsAuthenticated(false);
  }, []);

  return (
    <AuthContext.Provider
      value={{ isAuthenticated, isLoading, userRole, userId, login, logout, setUnauthenticated }}
    >
      {children}
    </AuthContext.Provider>
  );
}
