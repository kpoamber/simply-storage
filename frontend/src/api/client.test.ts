import { describe, it, expect } from 'vitest';
import apiClient from './client';

describe('apiClient', () => {
  it('has correct base URL', () => {
    expect(apiClient.defaults.baseURL).toBe('/api');
  });

  it('has JSON content type header', () => {
    expect(apiClient.defaults.headers['Content-Type']).toBe(
      'application/json',
    );
  });

  it('has response interceptors configured', () => {
    // axios interceptors have a handlers array
    expect(
      (apiClient.interceptors.response as unknown as { handlers: unknown[] })
        .handlers.length,
    ).toBeGreaterThan(0);
  });
});
