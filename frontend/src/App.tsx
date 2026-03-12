import { Routes, Route } from 'react-router-dom';
import Layout from './components/Layout';
import Dashboard from './pages/Dashboard';
import Projects from './pages/Projects';
import ProjectDetail from './pages/ProjectDetail';
import Storages from './pages/Storages';
import StorageDetail from './pages/StorageDetail';
import SyncTasks from './pages/SyncTasks';
import Nodes from './pages/Nodes';

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="projects" element={<Projects />} />
        <Route path="projects/:id" element={<ProjectDetail />} />
        <Route path="storages" element={<Storages />} />
        <Route path="storages/:id" element={<StorageDetail />} />
        <Route path="sync-tasks" element={<SyncTasks />} />
        <Route path="nodes" element={<Nodes />} />
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
