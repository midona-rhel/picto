import { describe, expect, it } from 'vitest';
import {
  extractRustCommandsFromText,
  extractTsCommandsFromText,
} from '../check-command-parity-lib.mjs';

describe('check-command-parity parser helpers', () => {
  it('extracts invoke commands with nested generics', () => {
    const source = `
      import { invoke } from '#desktop/api';
      await invoke<Array<Record<string, unknown>>>('list_smart_folders');
      await invoke<Foo<Bar<Baz>>>('create_smart_folder', { folder });
      await invoke('open_settings_window');
    `;

    const commands = extractTsCommandsFromText(source, 'fixture.ts');
    expect(commands.has('list_smart_folders')).toBe(true);
    expect(commands.has('create_smart_folder')).toBe(true);
    expect(commands.has('open_settings_window')).toBe(true);
  });

  it('extracts invoke commands when generic arguments span multiple lines', () => {
    const source = `
      import { invoke } from '#desktop/api';
      await invoke<
        Array<Record<string, unknown>>
      >(
        'update_smart_folder',
        { id, folder }
      );
    `;

    const commands = extractTsCommandsFromText(source, 'fixture.ts');
    expect(commands.has('update_smart_folder')).toBe(true);
  });

  it('extracts multiple rust command literals on one match arm', () => {
    const source = `
      "approve" | "reject" => cmd_result_ok(()),
      "ptr_bootstrap_from_hydrus_snapshot" => cmd_result_ok(()),
      let not_a_command = "hello";
    `;

    const commands = extractRustCommandsFromText(source);
    expect(commands.has('approve')).toBe(true);
    expect(commands.has('reject')).toBe(true);
    expect(commands.has('ptr_bootstrap_from_hydrus_snapshot')).toBe(true);
    expect(commands.has('hello')).toBe(false);
  });
});
