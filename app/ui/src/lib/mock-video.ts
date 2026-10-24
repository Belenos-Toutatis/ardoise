/**
 * Browser stand-in for the video export and OpenH264 commands (`estimate_video_export`,
 * `export_video`, `cancel_video_export`, `openh264_*`, `set_openh264_allowed`).
 *
 * Contract stub: filled in by the video export work package.
 */
import { UNHANDLED } from './mock-data';
import type { ExportSource } from './note';
import type {
  DownloadProgress,
  OpenH264State,
  VideoExportOptions,
  VideoExportResult,
  VideoProgress,
} from './video';

export function mockVideoInvoke(
  command: string,
  args: Record<string, unknown>,
): unknown {
  void command;
  void args;
  return UNHANDLED;
}

export function mockVideoExport(
  source: ExportSource,
  options: VideoExportOptions,
  destination: string,
  onProgress: (progress: VideoProgress) => void,
): Promise<VideoExportResult> {
  void source;
  void options;
  void onProgress;
  return Promise.reject({
    code: 'videoExportFailed',
    message: `not implemented: ${destination}`,
  });
}

export function mockOpenH264Download(
  onProgress: (progress: DownloadProgress) => void,
): Promise<OpenH264State> {
  void onProgress;
  return Promise.reject({
    code: 'openh264DownloadFailed',
    message: 'not implemented',
  });
}
