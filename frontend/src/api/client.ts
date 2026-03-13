import axios from 'axios';

const apiClient = axios.create({
  baseURL: '/api',
  headers: {
    'Content-Type': 'application/json',
  },
});

let getAccessToken: (() => string | null) | null = null;
let onRefreshToken: (() => Promise<string | null>) | null = null;

export function setAuthInterceptors(
  tokenGetter: () => string | null,
  refresher: () => Promise<string | null>,
) {
  getAccessToken = tokenGetter;
  onRefreshToken = refresher;
}

// Request interceptor: add Authorization header
apiClient.interceptors.request.use((config) => {
  const token = getAccessToken?.();
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

// Response interceptor: handle errors and 401 refresh
let isRefreshing = false;
let failedQueue: Array<{
  resolve: (token: string | null) => void;
  reject: (error: unknown) => void;
}> = [];

function processQueue(error: unknown, token: string | null) {
  failedQueue.forEach(({ resolve, reject }) => {
    if (error) {
      reject(error);
    } else {
      resolve(token);
    }
  });
  failedQueue = [];
}

apiClient.interceptors.response.use(
  (response) => response,
  async (error) => {
    const originalRequest = error.config;

    if (
      error.response?.status === 401 &&
      onRefreshToken &&
      !originalRequest._retry &&
      !originalRequest.url?.match(/\/auth\/(login|refresh|logout)$/)
    ) {
      if (isRefreshing) {
        return new Promise((resolve, reject) => {
          failedQueue.push({
            resolve: (token) => {
              originalRequest.headers.Authorization = `Bearer ${token}`;
              resolve(apiClient(originalRequest));
            },
            reject,
          });
        });
      }

      originalRequest._retry = true;
      isRefreshing = true;

      try {
        const newToken = await onRefreshToken();
        if (newToken) {
          processQueue(null, newToken);
          originalRequest.headers.Authorization = `Bearer ${newToken}`;
          return apiClient(originalRequest);
        } else {
          processQueue(new Error('Refresh failed'), null);
          return Promise.reject(error);
        }
      } catch (refreshError) {
        processQueue(refreshError, null);
        return Promise.reject(refreshError);
      } finally {
        isRefreshing = false;
      }
    }

    if (error.response) {
      const message = error.response.data?.error || error.response.statusText;
      console.error(`API error ${error.response.status}: ${message}`);
    } else if (error.request) {
      console.error('Network error: no response received');
    }
    return Promise.reject(error);
  },
);

export async function uploadFile(
  projectId: string,
  file: File,
  metadata?: Record<string, string | number | boolean>,
) {
  const formData = new FormData();
  formData.append('file', file);
  if (metadata && Object.keys(metadata).length > 0) {
    formData.append('metadata', JSON.stringify(metadata));
  }
  return apiClient.post(`/projects/${projectId}/files`, formData, {
    headers: { 'Content-Type': 'multipart/form-data' },
  });
}

export async function searchFiles(projectId: string, request: import('./types').SearchRequest) {
  return apiClient.post<import('./types').SearchResult>(
    `/projects/${projectId}/files/search`,
    request,
  );
}

export async function searchSummary(projectId: string, filters?: import('./types').MetadataFilterNode) {
  return apiClient.post<import('./types').SearchSummary>(
    `/projects/${projectId}/files/search/summary`,
    filters ? { filters } : {},
  );
}

export async function bulkDeletePreview(projectId: string, filters: import('./types').BulkDeleteRequest) {
  return apiClient.post<import('./types').BulkDeletePreview>(
    `/projects/${projectId}/files/bulk-delete/preview`,
    filters,
  );
}

export async function bulkDeleteExecute(projectId: string, filters: import('./types').BulkDeleteRequest) {
  return apiClient.post<import('./types').BulkDeleteResult>(
    `/projects/${projectId}/files/bulk-delete`,
    filters,
  );
}

export default apiClient;
