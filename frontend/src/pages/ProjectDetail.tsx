import { useParams } from 'react-router-dom';

export default function ProjectDetail() {
  const { id } = useParams<{ id: string }>();

  return (
    <div>
      <h2 className="text-2xl font-semibold text-gray-800">Project Detail</h2>
      <p className="mt-2 text-gray-500">Project ID: {id}</p>
    </div>
  );
}
