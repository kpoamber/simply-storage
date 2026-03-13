import { Routes, Route, Navigate } from 'react-router-dom';
import { AuthProvider, useAuth } from './contexts/AuthContext';
import Layout from './components/Layout';
import Login from './pages/Login';
import Dashboard from './pages/Dashboard';
import Projects from './pages/Projects';
import ProjectDetail from './pages/ProjectDetail';
import Storages from './pages/Storages';
import StorageDetail from './pages/StorageDetail';
import SyncTasks from './pages/SyncTasks';
import Nodes from './pages/Nodes';
import Users from './pages/Users';
import UserDetail from './pages/UserDetail';

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { user, isLoading } = useAuth();

  if (isLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-50">
        <p className="text-gray-500">Loading...</p>
      </div>
    );
  }

  if (!user) {
    return <Navigate to="/login" replace />;
  }

  return <>{children}</>;
}

function AdminRoute({ children }: { children: React.ReactNode }) {
  const { user } = useAuth();

  if (user?.role !== 'admin') {
    return <Navigate to="/" replace />;
  }

  return <>{children}</>;
}

function AppRoutes() {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route
        path="/"
        element={
          <ProtectedRoute>
            <Layout />
          </ProtectedRoute>
        }
      >
        <Route index element={<Dashboard />} />
        <Route path="projects" element={<Projects />} />
        <Route path="projects/:id" element={<ProjectDetail />} />
        <Route path="storages" element={<Storages />} />
        <Route path="storages/:id" element={<StorageDetail />} />
        <Route path="sync-tasks" element={<SyncTasks />} />
        <Route path="nodes" element={<Nodes />} />
        <Route path="users" element={<AdminRoute><Users /></AdminRoute>} />
        <Route path="users/:id" element={<AdminRoute><UserDetail /></AdminRoute>} />
        <Route
          path="*"
          element={
            <div className="text-center py-12">
              <h2 className="text-2xl font-semibold text-gray-600">
                404 - Page Not Found
              </h2>
            </div>
          }
        />
      </Route>
    </Routes>
  );
}

export default function App() {
  return (
    <AuthProvider>
      <AppRoutes />
    </AuthProvider>
  );
}
