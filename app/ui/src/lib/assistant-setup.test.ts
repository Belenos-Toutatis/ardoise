import { describe, expect, it } from 'vitest';
import { claudeCodeCommand, claudeDesktopConfig, mcpArgs } from './assistant-setup';

const EXE = 'C:\\Users\\Prof Physique\\Outils\\Ardoise\\ardoise.exe';

describe('declaring Ardoise in an assistant', () => {
  it('Claude Code: user scope, the quoted path after a quoted --', () => {
    expect(claudeCodeCommand(EXE, false)).toBe(
      'claude mcp add --scope user ardoise "--" "C:\\Users\\Prof Physique\\Outils\\Ardoise\\ardoise.exe" --mcp',
    );
    expect(claudeCodeCommand(EXE, true)).toMatch(/ardoise\.exe" --mcp --read-only$/);
  });

  it('Claude Desktop: an mcpServers entry, valid JSON with the path as it is', () => {
    const text = claudeDesktopConfig(EXE, true);
    expect(JSON.parse(text)).toEqual({ mcpServers: { ardoise: { command: EXE, args: ['--mcp', '--read-only'] } } });
    // Backslashes are doubled in the JSON text.
    expect(text).toContain(JSON.stringify(EXE));
    expect(mcpArgs(false)).toEqual(['--mcp']);
  });
});
