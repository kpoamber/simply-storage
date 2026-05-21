import Uppy from '@uppy/core';
import Tus from '@uppy/tus';
import { getCurrentAccessToken } from '../api/client';

/// Files at or below this size use the legacy single-request upload; larger
/// files are chunked through the resumable tus endpoint to stay under the
/// Cloudflare 100 MB request-body limit.
export const LARGE_FILE_THRESHOLD = 90 * 1024 * 1024;

/// Chunk size for resumable uploads — comfortably under the 100 MB CF limit.
const CHUNK_SIZE = 48 * 1024 * 1024;

export interface TusUploadParams {
  projectId: string;
  file: File;
  metadata: Record<string, string>;
  onProgress?: (pct: number) => void;
}

/// Upload a single large file via tus (Uppy core + @uppy/tus). Resolves on
/// success, rejects on failure. One disposable Uppy instance per file keeps the
/// integration self-contained alongside the existing custom drop zone.
export function uploadViaTus(params: TusUploadParams): Promise<void> {
  return new Promise((resolve, reject) => {
    const uppy = new Uppy({ autoProceed: true, allowMultipleUploadBatches: false });

    uppy.use(Tus, {
      endpoint: `/api/projects/${params.projectId}/uploads`,
      chunkSize: CHUNK_SIZE,
      removeFingerprintOnSuccess: true,
      retryDelays: [0, 1000, 3000, 5000],
      // Inject the token per request (not once per upload) so long-running
      // chunked uploads keep using a fresh token as AuthContext refreshes it.
      onBeforeRequest: (req) => {
        const token = getCurrentAccessToken();
        if (token) {
          req.setHeader('Authorization', `Bearer ${token}`);
        }
      },
    });

    // Our backend reads app metadata from the tus "meta" key (base64 JSON).
    uppy.setMeta({ meta: JSON.stringify(params.metadata) });

    uppy.on('upload-progress', (_file, progress) => {
      if (progress.bytesTotal) {
        params.onProgress?.(Math.round((progress.bytesUploaded / progress.bytesTotal) * 100));
      }
    });

    uppy.on('complete', (result) => {
      const failed = result.failed ?? [];
      uppy.destroy();
      if (failed.length > 0) {
        reject(new Error(failed[0]?.error || 'tus upload failed'));
      } else {
        resolve();
      }
    });

    uppy.on('error', (err) => {
      uppy.destroy();
      reject(err);
    });

    try {
      uppy.addFile({ name: params.file.name, type: params.file.type, data: params.file });
    } catch (err) {
      uppy.destroy();
      reject(err as Error);
    }
  });
}
