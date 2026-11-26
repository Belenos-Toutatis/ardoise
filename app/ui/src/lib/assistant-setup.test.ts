import { describe, expect, it } from 'vitest';
import { claudeCodeCommand, claudeDesktopConfig, mcpArgs, pathChangedByShell } from './assistant-setup';

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

describe('a path the shells would change inside double quotes', () => {
  it.each([
    ['C:\Users\prof$1\ardoise.exe', true],
    ['C:\Users\prof`s\ardoise.exe', true],
    ['C:\Users\%USERNAME%\ardoise.exe', true],
    [EXE, false],
    ['C:\Users\Prof (école)\Ardoise\ardoise.exe', false],
  ])('%s: %s', (exe, risky) => {
    expect(pathChangedByShell(exe)).toBe(risky);
  });
});
