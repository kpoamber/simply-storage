import { Outlet } from 'react-router-dom';
import Sidebar from './Sidebar';
import { useAuth } from '../contexts/AuthContext';
import { LogOut } from 'lucide-react';

export default function Layout() {
  const { user, logout } = useAuth();

  return (
    <div className="flex min-h-screen bg-gray-50">
      <Sidebar />
      <div className="flex-1 flex flex-col">
        <header className="h-12 border-b border-gray-200 bg-white flex items-center justify-end px-6">
          {user && (
            <div className="flex items-center gap-3">
              <span className="text-sm text-gray-600">
                {user.username}
                <span className="ml-1 text-xs text-gray-400">
                  ({user.role})
                </span>
              </span>
              <button
                onClick={logout}
                title="Logout"
                className="p-1.5 text-gray-400 hover:text-gray-600 rounded transition-colors"
              >
                <LogOut size={16} />
              </button>
            </div>
          )}
        </header>
        <main className="flex-1 p-6 overflow-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
