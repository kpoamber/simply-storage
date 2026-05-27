import { NavLink } from 'react-router-dom';
import {
  LayoutDashboard,
  FolderOpen,
  HardDrive,
  RefreshCw,
  Server,
  Users,
  DatabaseBackup,
} from 'lucide-react';
import { useAuth } from '../contexts/AuthContext';
import ThemeToggle from './ThemeToggle';

const navItems = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard, adminOnly: false },
  { to: '/projects', label: 'Projects', icon: FolderOpen, adminOnly: false },
  { to: '/storages', label: 'Storages', icon: HardDrive, adminOnly: false },
  { to: '/sync-tasks', label: 'Sync Tasks', icon: RefreshCw, adminOnly: true },
  { to: '/nodes', label: 'Nodes', icon: Server, adminOnly: true },
  { to: '/users', label: 'Users', icon: Users, adminOnly: true },
  { to: '/backups', label: 'Backups', icon: DatabaseBackup, adminOnly: true },
];

export default function Sidebar() {
  const { user } = useAuth();
  const isAdmin = user?.role === 'admin';

  const visibleItems = navItems.filter(
    (item) => !item.adminOnly || isAdmin,
  );

  return (
    <aside className="flex w-[232px] min-h-screen flex-col border-r border-line bg-canvas text-ink">
      <div className="flex items-center justify-between px-4 py-4">
        <div className="flex items-center gap-2.5">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-ink">
            <span className="font-serif italic text-white text-lg leading-none">S</span>
          </div>
          <div className="leading-tight">
            <p className="text-[15px] font-semibold text-ink">Simply Storage</p>
            <p className="text-[11px] text-ink-3">Admin Panel</p>
          </div>
        </div>
        <ThemeToggle />
      </div>

      <nav className="flex-1 space-y-0.5 px-2 py-2">
        {visibleItems.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            className={({ isActive }) =>
              `flex items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors ${
                isActive
                  ? 'bg-elev text-ink shadow-soft1'
                  : 'text-ink-2 hover:bg-sunk hover:text-ink'
              }`
            }
          >
            <Icon size={17} />
            {label}
          </NavLink>
        ))}
      </nav>
    </aside>
  );
}
