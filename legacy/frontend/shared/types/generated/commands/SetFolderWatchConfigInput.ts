export type SetFolderWatchConfigInput = {
  folder_id: number;
  watch_path: string;
  watch_enabled: boolean;
  watch_subfolders: boolean;
  watch_import_status_mode: string;
  import_existing_now: boolean;
};
