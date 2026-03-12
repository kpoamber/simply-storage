import { useQuery } from '@tanstack/react-query';
import apiClient from '../api/client';

interface Node {
  id: string;
  node_id: string;
  address: string;
  started_at: string;
  last_heartbeat: string;
  created_at: string;
}

export default function Nodes() {
  const { data: nodes, isLoading, error } = useQuery<Node[]>({
    queryKey: ['nodes'],
    queryFn: () => apiClient.get('/system/nodes').then(r => r.data),
    refetchInterval: 30000,
  });

  return (
    <div>
      <h2 className="text-2xl font-semibold text-gray-800">Nodes</h2>
      <p className="mt-2 text-gray-500">Active service nodes in the cluster.</p>

      {isLoading && <p className="mt-4 text-gray-400">Loading nodes...</p>}
      {error && <p className="mt-4 text-red-500">Failed to load nodes.</p>}

      {nodes && nodes.length === 0 && (
        <p className="mt-4 text-gray-400">No active nodes found.</p>
      )}

      {nodes && nodes.length > 0 && (
        <div className="mt-6 overflow-x-auto">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Node ID</th>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Address</th>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Started</th>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Last Heartbeat</th>
              </tr>
            </thead>
            <tbody className="bg-white divide-y divide-gray-200">
              {nodes.map((node) => (
                <tr key={node.id}>
                  <td className="px-6 py-4 whitespace-nowrap text-sm font-mono text-gray-900">{node.node_id}</td>
                  <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{node.address}</td>
                  <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    {new Date(node.started_at).toLocaleString()}
                  </td>
                  <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    {new Date(node.last_heartbeat).toLocaleString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
