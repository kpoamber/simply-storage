import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  type ReactNode,
} from 'react';
import apiClient, { setAuthInterceptors } from '../api/client';
import type { AuthUser } from '../api/types';

interface AuthContextType {
  user: AuthUser | null;
  isLoading: boolean;
  login: (username: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthContextType | null>(null);

const REFRESH_TOKEN_KEY = 'simply_refresh_token';
const LEGACY_REFRESH_TOKEN_KEY = 'innovare_refresh_token';

/// One-time migration: when the app starts, copy a refresh token from the old
/// localStorage key (pre-rebrand) to the new one and remove the old key. Lets
/// existing sessions survive the rename.
function migrateLegacyRefreshToken() {
  if (typeof localStorage === 'undefined') return;
  if (localStorage.getItem(REFRESH_TOKEN_KEY)) return;
  const legacy = localStorage.getItem(LEGACY_REFRESH_TOKEN_KEY);
  if (legacy) {
    localStorage.setItem(REFRESH_TOKEN_KEY, legacy);
    localStorage.removeItem(LEGACY_REFRESH_TOKEN_KEY);
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  // Run the legacy-key migration before any read of REFRESH_TOKEN_KEY happens.
  migrateLegacyRefreshToken();

  const [user, setUser] = useState<AuthUser | null>(null);
  const [accessToken, setAccessToken] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const refreshToken = useCallback(async (): Promise<string | null> => {
    const stored = localStorage.getItem(REFRESH_TOKEN_KEY);
    if (!stored) return null;

    try {
      const { data } = await apiClient.post('/auth/refresh', {
        refresh_token: stored,
      });
      setAccessToken(data.access_token);
      localStorage.setItem(REFRESH_TOKEN_KEY, data.refresh_token);
      return data.access_token;
    } catch {
      localStorage.removeItem(REFRESH_TOKEN_KEY);
      setAccessToken(null);
      setUser(null);
      return null;
    }
  }, []);

  // Set up interceptors once
  useEffect(() => {
    setAuthInterceptors(() => accessToken, refreshToken);
  }, [accessToken, refreshToken]);

  // Proactively refresh the access token well before its TTL (default 15 min).
  // Without this, only the axios 401 interceptor refreshes — but long-running
  // tus uploads bypass axios and would 401 mid-upload once the token expires.
  useEffect(() => {
    if (!accessToken) return;
    const interval = setInterval(() => {
      refreshToken();
    }, 10 * 60 * 1000); // every 10 min
    return () => clearInterval(interval);
  }, [accessToken, refreshToken]);

  // Try to restore session on mount
  useEffect(() => {
    const restore = async () => {
      const stored = localStorage.getItem(REFRESH_TOKEN_KEY);
      if (!stored) {
        setIsLoading(false);
        return;
      }

      const token = await refreshToken();
      if (token) {
        try {
          const { data } = await apiClient.get('/auth/me', {
            headers: { Authorization: `Bearer ${token}` },
          });
          setUser(data);
        } catch {
          localStorage.removeItem(REFRESH_TOKEN_KEY);
          setAccessToken(null);
        }
      }
      setIsLoading(false);
    };
    restore();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const login = useCallback(
    async (username: string, password: string) => {
      const { data } = await apiClient.post('/auth/login', {
        username,
        password,
      });
      setAccessToken(data.access_token);
      localStorage.setItem(REFRESH_TOKEN_KEY, data.refresh_token);

      const meResp = await apiClient.get('/auth/me', {
        headers: { Authorization: `Bearer ${data.access_token}` },
      });
      setUser(meResp.data);
    },
    [],
  );

  const logout = useCallback(async () => {
    const stored = localStorage.getItem(REFRESH_TOKEN_KEY);
    if (stored) {
      try {
        await apiClient.post('/auth/logout', { refresh_token: stored });
      } catch {
        // Ignore logout API errors
      }
    }
    localStorage.removeItem(REFRESH_TOKEN_KEY);
    setAccessToken(null);
    setUser(null);
  }, []);

  return (
    <AuthContext.Provider value={{ user, isLoading, login, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthContextType {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}
