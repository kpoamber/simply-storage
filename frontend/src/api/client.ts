import axios from 'axios';

const apiClient = axios.create({
  baseURL: '/api',
  headers: {
    'Content-Type': 'application/json',
  },
});

apiClient.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response) {
      const message = error.response.data?.error || error.response.statusText;
      console.error(`API error ${error.response.status}: ${message}`);
    } else if (error.request) {
      console.error('Network error: no response received');
    }
    return Promise.reject(error);
  },
);

export default apiClient;
