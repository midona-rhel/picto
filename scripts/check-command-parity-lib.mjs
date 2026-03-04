import ts from 'typescript';

// Match ALL command string literals on a match arm line.
// Handles "cmd_a" | "cmd_b" => patterns by extracting every quoted identifier.
const RUST_CMD_LINE_RE = /^\s*"[a-z_]+"\s*(?:\||\s*=>)/;
const RUST_CMD_TOKEN_RE = /"([a-z_]+)"/g;

/**
 * @param {string} content
 * @returns {Set<string>}
 */
export function extractRustCommandsFromText(content) {
  const commands = new Set();
  for (const line of content.split('\n')) {
    if (!RUST_CMD_LINE_RE.test(line)) continue;
    let match;
    RUST_CMD_TOKEN_RE.lastIndex = 0;
    while ((match = RUST_CMD_TOKEN_RE.exec(line)) !== null) {
      commands.add(match[1]);
    }
  }
  return commands;
}

/**
 * @param {string} content
 * @param {string} fileName
 * @returns {Set<string>}
 */
export function extractTsCommandsFromText(content, fileName = 'api.ts') {
  const commands = new Set();
  const sourceFile = ts.createSourceFile(
    fileName,
    content,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );

  /**
   * @param {ts.Node} node
   */
  const visit = (node) => {
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'invoke') {
      const [firstArg] = node.arguments;
      if (firstArg && ts.isStringLiteral(firstArg) && /^[a-z_]+$/.test(firstArg.text)) {
        commands.add(firstArg.text);
      }
    }
    ts.forEachChild(node, visit);
  };

  visit(sourceFile);
  return commands;
}
