/**
 * What to paste to declare this copy of Ardoise in an assistant (Settings > Assistant; guide
 * `docs/guide/assistant.*.md`). Ardoise never edits an assistant's configuration itself.
 *
 * Claude Code: `claude mcp add [options] <name> -- <command> [args...]`, everything after `--`
 * going to the server untouched; `--scope user` makes it available in every project
 * (https://code.claude.com/docs/en/mcp). Claude Desktop: an entry of `mcpServers` in
 * `claude_desktop_config.json` (https://modelcontextprotocol.io/docs/develop/connect-local-servers).
 */

/** The name of the server in the assistant (`/mcp__ardoise__…` commands, tools `mcp__ardoise__…`). */
export const SERVER_NAME = 'ardoise';

/** Arguments of `ardoise.exe` for the MCP server. */
export function mcpArgs(readOnly: boolean): string[] {
  return readOnly ? ['--mcp', '--read-only'] : ['--mcp'];
}

/**
 * The command for Claude Code, for PowerShell or the command prompt: the path in double quotes
 * (it may hold spaces; Windows paths never hold a double quote).
 */
export function claudeCodeCommand(exe: string, readOnly: boolean): string {
  return `claude mcp add --scope user ${SERVER_NAME} -- "${exe}" ${mcpArgs(readOnly).join(' ')}`;
}

/** The JSON for Claude Desktop (backslashes doubled by JSON itself). */
export function claudeDesktopConfig(exe: string, readOnly: boolean): string {
  return JSON.stringify({ mcpServers: { [SERVER_NAME]: { command: exe, args: mcpArgs(readOnly) } } }, null, 2);
}
